use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::context::AppContext;
use crate::fs_util;
use crate::skill_fs::{self, SkillFile};
use crate::types::{ArtifactKind, LockEntry, LockSource};

use super::{InstallPlan, PreparedWrites, TargetAction, ToolIdentity};

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compare two semver version strings.
///
/// - `None` installed → `Less` (treat as "not installed").
/// - Both parse → standard semver comparison.
/// - Either parse fails → string equality: `Equal` if equal, else `Less`.
fn compare_versions(installed: Option<&str>, bundled: &str) -> Ordering {
    let Some(inst) = installed else {
        return Ordering::Less;
    };
    match (semver::Version::parse(inst), semver::Version::parse(bundled)) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => {
            if inst == bundled {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
    }
}

/// Decide what action to take for a platform that already has a lock entry.
pub(super) fn decide_action_for_entry(
    entry: &LockEntry,
    bundled_version: &str,
    source_checksum: &str,
    force: bool,
    skill_dest: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Result<TargetAction> {
    let installed_version = entry.version.as_deref();
    let cmp = compare_versions(installed_version, bundled_version);

    match cmp {
        Ordering::Less => Ok(TargetAction::Update {
            from: installed_version.map(str::to_string),
        }),
        Ordering::Equal => {
            if !ctx.fs.exists(skill_dest) {
                return Ok(TargetAction::Install);
            }

            let disk_checksum =
                checksum::checksum_artifact(skill_dest, ArtifactKind::Skill, ctx.fs)?;
            if disk_checksum == source_checksum {
                Ok(TargetAction::Skip)
            } else if force {
                Ok(TargetAction::Update {
                    from: installed_version.map(str::to_string),
                })
            } else {
                Ok(TargetAction::DriftedSkip {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
        Ordering::Greater => {
            if force {
                Ok(TargetAction::Downgrade {
                    from: installed_version.unwrap_or("unknown").to_string(),
                })
            } else {
                Ok(TargetAction::RefuseNewer {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
    }
}

fn discarded_paths_against_bundle(
    skill_dest: &std::path::Path,
    bundled_files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<Vec<std::path::PathBuf>> {
    if !ctx.fs.exists(skill_dest) {
        return Ok(Vec::new());
    }

    let installed_files = fs_util::collect_files_recursive(skill_dest, ctx.fs)?;
    let mut installed_by_rel = BTreeMap::new();
    for path in installed_files {
        let rel = path.strip_prefix(skill_dest).unwrap_or(&path).to_path_buf();
        installed_by_rel.insert(rel, ctx.fs.read(&path)?);
    }

    let mut bundled_by_rel = BTreeMap::new();
    for file in skill_fs::canonical_files(bundled_files) {
        bundled_by_rel.insert(file.rel_path.clone(), file.bytes.clone());
    }

    let mut changed_paths = Vec::new();
    let relative_paths: BTreeSet<_> =
        installed_by_rel.keys().chain(bundled_by_rel.keys()).cloned().collect();

    for rel_path in relative_paths {
        match (installed_by_rel.get(&rel_path), bundled_by_rel.get(&rel_path)) {
            (Some(installed), Some(bundled)) if installed == bundled => {}
            (Some(_) | None, Some(_)) | (Some(_), None) => {
                changed_paths.push(skill_dest.join(rel_path));
            }
            (None, None) => {}
        }
    }

    Ok(changed_paths)
}

pub(super) fn prepare_writes(
    plan: &InstallPlan,
    files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<PreparedWrites> {
    let mut dirs_to_write = BTreeSet::new();
    let mut dirs_to_replace = BTreeSet::new();

    for target in &plan.targets {
        if target.action.will_write() {
            dirs_to_write.insert(target.dest_dir.clone());
        }
        if plan.force
            && matches!(target.action, TargetAction::Update { .. } | TargetAction::Downgrade { .. })
        {
            dirs_to_replace.insert(target.dest_dir.clone());
        }
    }

    let mut discarded_paths_by_dir = BTreeMap::new();
    for dir in &dirs_to_replace {
        discarded_paths_by_dir
            .insert(dir.clone(), discarded_paths_against_bundle(dir, files, ctx)?);
        if ctx.fs.exists(dir) {
            ctx.fs.remove_dir_all(dir)?;
        }
    }

    Ok(PreparedWrites {
        dirs_to_write,
        discarded_paths_by_dir,
    })
}

pub(super) fn build_lock_entry(
    tool: &ToolIdentity,
    checksum: &str,
    installed_at: &str,
) -> LockEntry {
    LockEntry {
        artifact_type: ArtifactKind::Skill,
        version: Some(tool.version.clone()),
        installed_at: installed_at.to_string(),
        source: LockSource {
            repo: format!("bundled:{}", tool.name),
            path: format!("skills/{}", tool.name),
        },
        source_checksum: checksum.to_string(),
        installed_checksum: checksum.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::path::PathBuf;

    use crate::platform::Platform;
    use crate::skill_fs::SkillFile;
    use crate::skill_install::{InstallPlan, TargetAction, TargetPlan, ToolIdentity};
    use crate::test_support::TestContext;
    use crate::types::{ArtifactKind, InstallScope, LockEntry, LockSource};

    use super::{build_lock_entry, compare_versions, decide_action_for_entry, prepare_writes};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_entry(version: Option<&str>, checksum: &str) -> LockEntry {
        LockEntry {
            artifact_type: ArtifactKind::Skill,
            version: version.map(str::to_string),
            installed_at: "2024-01-01T00:00:00Z".to_string(),
            source: LockSource {
                repo: "bundled:sample".to_string(),
                path: "skills/sample".to_string(),
            },
            source_checksum: checksum.to_string(),
            installed_checksum: checksum.to_string(),
        }
    }

    fn skill_file(rel: &str, content: &str) -> SkillFile {
        SkillFile {
            rel_path: PathBuf::from(rel),
            bytes: content.as_bytes().to_vec(),
        }
    }

    fn make_plan(dest_dir: PathBuf, action: TargetAction, force: bool) -> InstallPlan {
        InstallPlan {
            tool: ToolIdentity::new("sample", "1.0.0"),
            scope: InstallScope::Global,
            source_checksum: "sha256:abc".to_string(),
            cmx_present: false,
            force,
            targets: vec![TargetPlan {
                platform: Platform::Claude,
                scope: InstallScope::Global,
                dest_dir,
                files: vec![],
                action,
                cmx_managed: false,
            }],
        }
    }

    // -----------------------------------------------------------------------
    // compare_versions
    // -----------------------------------------------------------------------

    #[test]
    fn compare_versions_none_installed_is_less() {
        assert_eq!(compare_versions(None, "1.0.0"), Ordering::Less);
    }

    #[test]
    fn compare_versions_semver_1_10_0_greater_than_1_9_0() {
        // String-wise "1.10.0" < "1.9.0" — semver must give Greater, not Less.
        assert_eq!(
            compare_versions(Some("1.10.0"), "1.9.0"),
            Ordering::Greater,
            "semver ordering must beat lexicographic ordering"
        );
    }

    #[test]
    fn compare_versions_semver_equal_versions() {
        assert_eq!(compare_versions(Some("1.0.0"), "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn compare_versions_semver_older_installed_is_less() {
        assert_eq!(compare_versions(Some("0.9.0"), "1.0.0"), Ordering::Less);
    }

    #[test]
    fn compare_versions_non_semver_equal_strings_is_equal() {
        assert_eq!(compare_versions(Some("v1-alpha"), "v1-alpha"), Ordering::Equal);
    }

    #[test]
    fn compare_versions_non_semver_different_strings_is_less() {
        assert_eq!(compare_versions(Some("v1-alpha"), "v2-beta"), Ordering::Less);
    }

    // -----------------------------------------------------------------------
    // decide_action_for_entry — all seven branches
    // -----------------------------------------------------------------------

    #[test]
    fn decide_action_older_installed_produces_update_with_from() {
        let t = TestContext::new();
        let entry = make_entry(Some("0.9.0"), "sha256:old");
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:new", false, &dest, &ctx).unwrap();
        match action {
            TargetAction::Update { from } => assert_eq!(from.as_deref(), Some("0.9.0")),
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn decide_action_none_version_installed_produces_update_with_none_from() {
        let t = TestContext::new();
        let entry = make_entry(None, "sha256:old");
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:new", false, &dest, &ctx).unwrap();
        match action {
            TargetAction::Update { from } => {
                assert!(from.is_none(), "from must be None when no version was installed");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn decide_action_equal_version_dest_absent_produces_install() {
        let t = TestContext::new();
        let entry = make_entry(Some("1.0.0"), "sha256:abc");
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        // dest does NOT exist in the fake filesystem
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:abc", false, &dest, &ctx).unwrap();
        assert!(matches!(action, TargetAction::Install), "expected Install, got {action:?}");
    }

    #[test]
    fn decide_action_equal_version_matching_checksum_produces_skip() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_file(dest.join("SKILL.md"), "---\ndescription: test\n---\n# skill\n");
        let on_disk =
            crate::checksum::checksum_artifact(&dest, ArtifactKind::Skill, &t.fs).unwrap();
        let entry = make_entry(Some("1.0.0"), &on_disk);
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", &on_disk, false, &dest, &ctx).unwrap();
        assert!(matches!(action, TargetAction::Skip), "expected Skip, got {action:?}");
    }

    #[test]
    fn decide_action_equal_version_checksum_mismatch_no_force_produces_drifted_skip() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_file(dest.join("SKILL.md"), "---\ndescription: modified\n---\n# modified\n");
        let entry = make_entry(Some("1.0.0"), "sha256:original");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:original", false, &dest, &ctx)
                .unwrap();
        match action {
            TargetAction::DriftedSkip { installed } => {
                assert_eq!(installed, "1.0.0", "installed field must carry the version string");
            }
            other => panic!("expected DriftedSkip, got {other:?}"),
        }
    }

    #[test]
    fn decide_action_equal_version_checksum_mismatch_force_produces_update() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_file(dest.join("SKILL.md"), "---\ndescription: modified\n---\n# modified\n");
        let entry = make_entry(Some("1.0.0"), "sha256:original");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:original", true, &dest, &ctx).unwrap();
        match action {
            TargetAction::Update { from } => {
                assert_eq!(from.as_deref(), Some("1.0.0"));
            }
            other => panic!("expected Update (force), got {other:?}"),
        }
    }

    #[test]
    fn decide_action_newer_installed_no_force_produces_refuse_newer() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_dir(dest.clone());
        let entry = make_entry(Some("2.0.0"), "sha256:new");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:old", false, &dest, &ctx).unwrap();
        match action {
            TargetAction::RefuseNewer { installed } => {
                assert_eq!(installed, "2.0.0");
            }
            other => panic!("expected RefuseNewer, got {other:?}"),
        }
    }

    #[test]
    fn decide_action_newer_installed_with_force_produces_downgrade() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_dir(dest.clone());
        let entry = make_entry(Some("2.0.0"), "sha256:new");
        let ctx = t.ctx();
        let action =
            decide_action_for_entry(&entry, "1.0.0", "sha256:old", true, &dest, &ctx).unwrap();
        match action {
            TargetAction::Downgrade { from } => {
                assert_eq!(from, "2.0.0");
            }
            other => panic!("expected Downgrade, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // prepare_writes
    // -----------------------------------------------------------------------

    #[test]
    fn prepare_writes_skip_target_produces_empty_sets() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        let plan = make_plan(dest, TargetAction::Skip, false);
        let ctx = t.ctx();
        let result = prepare_writes(&plan, &[], &ctx).unwrap();
        assert!(result.dirs_to_write.is_empty(), "Skip must not add to dirs_to_write");
        assert!(result.discarded_paths_by_dir.is_empty(), "Skip must not add discarded paths");
    }

    #[test]
    fn prepare_writes_non_force_update_adds_to_write_but_not_replace() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        let skill_md = dest.join("SKILL.md");
        t.fs.add_file(&skill_md, "existing content");
        let plan = make_plan(
            dest.clone(),
            TargetAction::Update {
                from: Some("0.9.0".to_string()),
            },
            false, // not force
        );
        let files = vec![skill_file("SKILL.md", "new content")];
        let ctx = t.ctx();
        let result = prepare_writes(&plan, &files, &ctx).unwrap();
        assert!(result.dirs_to_write.contains(&dest), "Update must add to dirs_to_write");
        assert!(
            result.discarded_paths_by_dir.is_empty(),
            "non-force Update must not replace the dir"
        );
        // Existing file must not have been removed
        assert!(ctx.fs.exists(&skill_md), "non-force Update must not remove existing files");
    }

    #[test]
    fn prepare_writes_force_update_reports_discarded_and_removes_dir() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        // Modified file — differs from bundle
        t.fs.add_file(dest.join("SKILL.md"), "---\ndescription: modified\n---\n# modified\n");
        // Identical file — matches bundle
        let identical = "print('hello')";
        t.fs.add_file(dest.join("scripts/tool.py"), identical);
        // Installed-only file — not in bundle
        t.fs.add_file(dest.join("local-notes.md"), "scratch");

        let plan = make_plan(
            dest.clone(),
            TargetAction::Update {
                from: Some("1.0.0".to_string()),
            },
            true, // force
        );
        let files = vec![
            skill_file("SKILL.md", "---\ndescription: original\n---\n# original\n"),
            skill_file("scripts/tool.py", identical),
        ];
        let ctx = t.ctx();
        let result = prepare_writes(&plan, &files, &ctx).unwrap();

        assert!(result.dirs_to_write.contains(&dest));
        let discarded = result
            .discarded_paths_by_dir
            .get(&dest)
            .expect("force Update must report discarded paths");
        let discarded: std::collections::BTreeSet<_> = discarded.iter().cloned().collect();
        assert!(
            discarded.contains(&dest.join("SKILL.md")),
            "modified SKILL.md must appear in discarded"
        );
        assert!(
            discarded.contains(&dest.join("local-notes.md")),
            "installed-only file must appear in discarded"
        );
        assert!(
            !discarded.contains(&dest.join("scripts/tool.py")),
            "byte-identical file must NOT appear in discarded"
        );
        // Dir removed on force
        assert!(!ctx.fs.exists(&dest.join("SKILL.md")), "force Update must remove the dest dir");
    }

    // -----------------------------------------------------------------------
    // build_lock_entry
    // -----------------------------------------------------------------------

    #[test]
    fn build_lock_entry_repo_uses_bundled_prefix() {
        let tool = ToolIdentity::new("cmx", "1.2.3");
        let entry = build_lock_entry(&tool, "sha256:abc", "2024-01-01T00:00:00Z");
        assert_eq!(
            entry.source.repo, "bundled:cmx",
            "repo must be 'bundled:<name>' — a typo here breaks lockfile round-trips"
        );
        assert_eq!(entry.source.path, "skills/cmx", "path must be 'skills/<name>'");
    }

    #[test]
    fn build_lock_entry_source_and_installed_checksums_match() {
        let tool = ToolIdentity::new("cmx", "1.0.0");
        let entry = build_lock_entry(&tool, "sha256:deadbeef", "2024-01-01T00:00:00Z");
        assert_eq!(entry.source_checksum, "sha256:deadbeef");
        assert_eq!(
            entry.installed_checksum, entry.source_checksum,
            "fresh install: installed_checksum must equal source_checksum"
        );
        assert_eq!(entry.artifact_type, ArtifactKind::Skill);
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
    }
}
