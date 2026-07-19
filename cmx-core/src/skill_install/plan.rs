use crate::error::{CmxError, Result};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::frontmatter;
use crate::fs_util;
use crate::lockfile;
use crate::platform_iter;
use crate::skill_fs::{self, SkillFile};
use crate::targets;
use crate::types::{ArtifactKind, LockEntry, LockSource};

use super::{
    BundledSkill, InstallPlan, PlannedFile, PreparedWrites, Scope, SkillInstaller, TargetAction,
    TargetPlan, ToolIdentity,
};

// ---------------------------------------------------------------------------
// plan() method
// ---------------------------------------------------------------------------

impl SkillInstaller {
    /// Compute a dry-run install plan without writing anything.
    ///
    /// Fails if the bundle does not contain a `SKILL.md`.
    pub fn plan(
        &self,
        skill: &BundledSkill,
        scope: Scope,
        force: bool,
        ctx: &AppContext<'_>,
    ) -> Result<InstallPlan> {
        if !skill.has_skill_md() {
            return Err(CmxError::MissingSkillMd {
                skill: self.tool.name.clone(),
            });
        }

        // Reconcile the SKILL.md frontmatter's `metadata.version` to this tool's
        // version before anything else, so the checksum, the written bytes, and the
        // lock entry all describe the same, version-stamped content.
        let files = frontmatter::reconcile_skill_version(&skill.files, &self.tool.version);
        let source_checksum = skill_fs::checksum_bundled(&files);
        let install_scope = scope.to_install_scope();

        let platform_targets =
            targets::resolve_targets(None, ArtifactKind::Skill, install_scope, ctx)?;

        let cmx_managed = config::managed_platforms(ctx.fs, ctx.paths)?.is_some();
        let cmx_present = cmx_managed || {
            // Check whether any platform has a non-empty lock file
            platform_iter::views_for(ctx.paths, platform_iter::all(), ArtifactKind::Skill).any(
                |view| {
                    lockfile::load(install_scope, ctx.fs, &view.paths)
                        .ok()
                        .is_some_and(|l| !l.packages.is_empty())
                },
            )
        };

        let mut target_plans = Vec::new();

        for &platform in &platform_targets {
            let pv = ctx.paths.with_platform(platform);
            let dest_dir = pv.require_install_dir(ArtifactKind::Skill, install_scope)?;
            let skill_dest = dest_dir.join(&self.tool.name);

            // Build planned files
            let planned_files: Vec<PlannedFile> = files
                .iter()
                .map(|f| PlannedFile {
                    rel_path: f.rel_path.clone(),
                    dest_path: skill_dest.join(&f.rel_path),
                })
                .collect();

            // Determine the action for this platform
            let lock = lockfile::load(install_scope, ctx.fs, &pv)?;
            let action = if let Some(entry) = lock.packages.get(&self.tool.name) {
                decide_action_for_entry(
                    entry,
                    &self.tool.version,
                    &source_checksum,
                    force,
                    &skill_dest,
                    ctx,
                )?
            } else if ctx.fs.exists(&skill_dest) {
                // On disk but not tracked: treat as Install (untracked copy)
                TargetAction::Install
            } else {
                TargetAction::Install
            };

            target_plans.push(TargetPlan {
                platform,
                scope: install_scope,
                dest_dir: skill_dest,
                files: planned_files,
                action,
                cmx_managed,
            });
        }

        Ok(InstallPlan {
            tool: self.tool.clone(),
            scope: install_scope,
            source_checksum,
            cmx_present,
            force,
            targets: target_plans,
        })
    }
}

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
pub(in crate::skill_install) fn decide_action_for_entry(
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

pub(in crate::skill_install) fn prepare_writes(
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

pub(in crate::skill_install) fn build_lock_entry(
    tool: &ToolIdentity,
    checksum: &str,
    installed_at: &str,
) -> LockEntry {
    LockEntry::new(
        ArtifactKind::Skill,
        Some(tool.version.clone()),
        LockSource::new(format!("bundled:{}", tool.name), format!("skills/{}", tool.name)),
        checksum.to_string(),
        checksum.to_string(),
        installed_at.to_string(),
    )
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

    use super::super::test_support::{
        installer, make_file, plan_with_locked_version, sample_skill,
    };
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
    // Tests 1-3: skill_fs / checksum parity
    // -----------------------------------------------------------------------

    #[test]
    fn checksum_bundled_matches_checksum_dir_after_write() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let expected = crate::skill_fs::checksum_bundled(&skill.files);
        let dest = std::path::PathBuf::from("/dest/sample");
        crate::skill_fs::write_skill_files(&dest, &skill.files, &t.fs).unwrap();
        let on_disk = crate::checksum::checksum_dir(&dest, &t.fs).unwrap();
        assert_eq!(expected, on_disk, "in-memory checksum must match disk checksum");
    }

    #[test]
    fn dotfiles_and_transient_excluded_from_write_and_checksum() {
        let files = vec![
            make_file("SKILL.md", "# skill"),
            make_file(".hidden", "hidden"),
            make_file("node_modules/dep.js", "vendor"),
        ];
        let bundled_cs = crate::skill_fs::checksum_bundled(&files);

        // The checksum must only include SKILL.md
        let only_skill = vec![make_file("SKILL.md", "# skill")];
        let expected_cs = crate::skill_fs::checksum_bundled(&only_skill);
        assert_eq!(
            bundled_cs, expected_cs,
            "dotfiles and transient must be excluded from checksum"
        );
    }

    #[test]
    fn write_skill_files_creates_nested_dirs() {
        let t = TestContext::new();
        let files = vec![
            make_file("SKILL.md", "# skill"),
            make_file("scripts/sub/tool.py", "code"),
        ];
        crate::skill_fs::write_skill_files(std::path::Path::new("/dest/s"), &files, &t.fs).unwrap();
        assert!(t.fs.file_exists(std::path::Path::new("/dest/s/SKILL.md")));
        assert!(t.fs.file_exists(std::path::Path::new("/dest/s/scripts/sub/tool.py")));
    }

    // -----------------------------------------------------------------------
    // Tests 4-6: plan() target selection
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_machine_produces_single_claude_target_install() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();

        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].platform, Platform::Claude);
        assert!(matches!(plan.targets[0].action, TargetAction::Install));
        assert!(!plan.cmx_present);
    }

    #[test]
    fn cmx_config_two_platforms_produces_two_targets_cmx_managed() {
        let t = TestContext::new();
        let cfg = crate::types::CmxConfig {
            platforms: vec![Platform::Claude, Platform::Codex],
            ..Default::default()
        };
        crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();

        let platforms: Vec<_> = plan.targets.iter().map(|t| t.platform).collect();
        assert!(platforms.contains(&Platform::Claude), "should include Claude");
        assert!(platforms.contains(&Platform::Codex), "should include Codex");
        assert!(plan.targets[0].cmx_managed, "cmx_managed should be true");
    }

    #[test]
    fn no_config_but_non_empty_codex_lock_targets_codex() {
        let t = TestContext::new();
        let codex_paths = t.paths.with_platform(Platform::Codex);
        crate::test_support::save_lock_with_entry(
            &t.fs,
            &codex_paths,
            "other-skill",
            crate::test_support::sample_lock_entry(),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();

        let platforms: Vec<_> = plan.targets.iter().map(|t| t.platform).collect();
        assert!(
            platforms.contains(&Platform::Codex),
            "Codex lock non-empty → should be targeted"
        );
    }

    // -----------------------------------------------------------------------
    // Tests 7-14: version-guard actions
    // -----------------------------------------------------------------------

    #[test]
    fn older_lock_version_produces_update() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "0.9.0", "sha256:old", "1.0.0", false);
        assert!(matches!(plan.targets[0].action, TargetAction::Update { .. }));
    }

    #[test]
    fn same_version_identical_checksum_on_disk_produces_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = crate::skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        crate::skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::Skip));
    }

    #[test]
    fn same_version_differing_content_no_force_produces_drifted_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = crate::skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::DriftedSkip { .. }));
    }

    #[test]
    fn same_version_differing_content_with_force_produces_update() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = crate::skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0")
            .plan(&skill, crate::skill_install::Scope::Global, true, &ctx)
            .unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::Update { .. }));
    }

    #[test]
    fn newer_lock_no_force_produces_refuse_newer_and_is_blocked() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "2.0.0", "sha256:new", "1.0.0", false);
        assert!(matches!(plan.targets[0].action, TargetAction::RefuseNewer { .. }));
        assert!(plan.is_blocked());
    }

    #[test]
    fn newer_lock_with_force_produces_downgrade() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "2.0.0", "sha256:new", "1.0.0", true);
        assert!(matches!(plan.targets[0].action, TargetAction::Downgrade { .. }));
        assert!(!plan.is_blocked());
    }

    #[test]
    fn non_semver_versions_use_string_equality_fallback() {
        let t = TestContext::new();
        let skill = sample_skill("v1-alpha");
        let checksum = crate::skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        crate::skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("v1-alpha".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("v1-alpha")
            .plan(&skill, crate::skill_install::Scope::Global, false, &ctx)
            .unwrap();
        assert!(
            matches!(plan.targets[0].action, TargetAction::Skip)
                || matches!(plan.targets[0].action, TargetAction::Install),
            "non-semver equal versions should not produce RefuseNewer or Downgrade"
        );
    }

    #[test]
    fn missing_skill_md_returns_error() {
        let t = TestContext::new();
        let skill = crate::skill_install::BundledSkill::from_files(vec![make_file(
            "scripts/tool.py",
            "code",
        )]);
        let ctx = t.ctx();
        let result =
            installer("1.0.0").plan(&skill, crate::skill_install::Scope::Global, false, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SKILL.md"));
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
            false,
        );
        let files = vec![skill_file("SKILL.md", "new content")];
        let ctx = t.ctx();
        let result = prepare_writes(&plan, &files, &ctx).unwrap();
        assert!(result.dirs_to_write.contains(&dest), "Update must add to dirs_to_write");
        assert!(
            result.discarded_paths_by_dir.is_empty(),
            "non-force Update must not replace the dir"
        );
        assert!(ctx.fs.exists(&skill_md), "non-force Update must not remove existing files");
    }

    #[test]
    fn prepare_writes_force_update_reports_discarded_and_removes_dir() {
        let t = TestContext::new();
        let dest = PathBuf::from("/home/testuser/.claude/skills/sample");
        t.fs.add_file(dest.join("SKILL.md"), "---\ndescription: modified\n---\n# modified\n");
        let identical = "print('hello')";
        t.fs.add_file(dest.join("scripts/tool.py"), identical);
        t.fs.add_file(dest.join("local-notes.md"), "scratch");

        let plan = make_plan(
            dest.clone(),
            TargetAction::Update {
                from: Some("1.0.0".to_string()),
            },
            true,
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
