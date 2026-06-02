use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Result;

use crate::context::AppContext;
use crate::lockfile;
use crate::partition::{Partitioned, partition_by};
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct UninstallResult {
    pub name: String,
    pub kind: ArtifactKind,
    pub scope: &'static str,
    /// Whether any lock entry was removed (the artifact was tracked somewhere).
    pub was_tracked: bool,
    /// Whether any physical copy was removed. `false` with `was_tracked` means we
    /// only reconciled stale lock entries (the files were already gone).
    pub was_on_disk: bool,
    /// The platforms whose lock entry was cleared.
    pub platforms: Vec<Platform>,
}

/// Result of uninstalling one or more named artifacts.
#[derive(Debug)]
pub struct BatchUninstallResult {
    pub kind: ArtifactKind,
    pub removed: Vec<UninstallResult>,
    /// Names that were not installed anywhere (nothing to remove).
    pub not_found: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Uninstall one artifact **everywhere cmx tracks it** at the given scope, across
/// every platform — returning `Ok(None)` when it isn't installed anywhere
/// (rather than erroring), so batch callers can report it without aborting.
///
/// `cmx doctor` presents a cross-platform, grouped view, so its inverse should
/// too: removing a skill deletes every physical copy and clears every platform's
/// lock entry for it. Because skills-only tools share one `.agents/skills`
/// directory, a single physical copy may be read by several tools; it is deleted
/// once, and each platform that *tracked* it has its lock entry cleared.
fn uninstall_one(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Option<UninstallResult>> {
    // Distinct physical locations to delete (the shared `.agents/skills` dir
    // resolves to the same path for several platforms — dedup so we delete once).
    let mut paths_to_delete: BTreeSet<PathBuf> = BTreeSet::new();
    let mut removed_from: Vec<Platform> = Vec::new();

    for platform in Platform::ALL {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let path = pv.installed_artifact_path(kind, name, scope);
        if ctx.fs.exists(&path) {
            paths_to_delete.insert(path);
        }
        if lockfile::load(scope, ctx.fs, &pv)?.packages.contains_key(name) {
            lockfile::mutate(scope, ctx.fs, &pv, |lock| {
                lock.packages.remove(name);
            })?;
            removed_from.push(platform);
        }
    }

    if paths_to_delete.is_empty() && removed_from.is_empty() {
        return Ok(None);
    }

    let was_on_disk = !paths_to_delete.is_empty();
    for path in &paths_to_delete {
        kind.remove_installed(path, ctx.fs)?;
    }

    removed_from.sort_by_key(|p| p.slug());
    removed_from.dedup();
    Ok(Some(UninstallResult {
        name: name.to_string(),
        kind,
        scope: scope.label(),
        was_tracked: !removed_from.is_empty(),
        was_on_disk,
        platforms: removed_from,
    }))
}

/// Uninstall a single named artifact, erroring if it isn't installed anywhere.
pub fn uninstall(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<UninstallResult> {
    uninstall_one(name, kind, scope, ctx)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No {kind} named '{name}' found in {} scope on any platform.",
            scope.label()
        )
    })
}

/// Uninstall several named artifacts in one pass. Best-effort: each name is
/// removed everywhere it's tracked; names not installed anywhere are collected
/// into `not_found` rather than aborting the batch.
pub fn uninstall_many(
    names: &[String],
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<BatchUninstallResult> {
    let (removed, not_found) = partition_by(names, |name| {
        Ok(match uninstall_one(name, kind, scope, ctx)? {
            Some(r) => Partitioned::Kept(r),
            None => Partitioned::Excluded(name.to_string()),
        })
    })?;
    Ok(BatchUninstallResult {
        kind,
        removed,
        not_found,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::platform::Platform;
    use crate::test_support::{TestContext, make_ctx, sample_lock_entry, test_paths_for};
    use crate::types::{ArtifactKind, InstallScope, LockFile};
    use std::collections::BTreeMap;

    // --- Display for UninstallResult ---

    #[test]
    fn uninstall_result_display_tracked() {
        let result = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: true,
            was_on_disk: true,
            platforms: vec![Platform::Claude],
        };
        let out = result.to_string();
        assert!(out.contains("Uninstalled my-agent"));
        assert!(!out.contains("untracked"));
    }

    #[test]
    fn uninstall_result_display_untracked() {
        let result = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: false,
            was_on_disk: true,
            platforms: vec![],
        };
        let out = result.to_string();
        assert!(out.contains("untracked"));
    }

    #[test]
    fn uninstall_bails_when_agent_not_installed() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let result = uninstall("nonexistent", ArtifactKind::Agent, InstallScope::Global, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("nonexistent"), "unexpected: {msg}");
    }

    #[test]
    fn uninstall_reconciles_tracked_but_absent_artifact() {
        // The "missing" case `cmx doctor` reports: a lock entry whose file is
        // already gone. uninstall must clear the stale entry, not bail.
        let t = TestContext::new();

        let mut packages = BTreeMap::new();
        packages.insert("skill-writing".to_string(), sample_lock_entry());
        let lock = LockFile {
            version: 1,
            packages,
        };
        lockfile::save(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();
        // Note: no file on disk for skill-writing.

        let ctx = t.ctx();
        let result =
            uninstall("skill-writing", ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();

        assert!(result.was_tracked, "the stale lock entry was tracked");
        assert!(!result.was_on_disk, "the file was already gone");

        let updated = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(
            !updated.packages.contains_key("skill-writing"),
            "stale lock entry should be removed"
        );
    }

    #[test]
    fn uninstall_bails_when_neither_on_disk_nor_tracked() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = uninstall("ghost", ArtifactKind::Skill, InstallScope::Global, &ctx);
        assert!(result.is_err(), "nothing on disk and no lock entry → bail");
        assert!(result.unwrap_err().to_string().contains("ghost"));
    }

    #[test]
    fn uninstall_removes_agent_file() {
        let t = TestContext::new();

        let agent_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        t.fs.add_file(agent_path.clone(), "# agent");

        let ctx = t.ctx();
        uninstall("my-agent", ArtifactKind::Agent, InstallScope::Global, &ctx).unwrap();

        assert!(!t.fs.file_exists(&agent_path), "agent file should be removed");
    }

    #[test]
    fn uninstall_removes_skill_dir() {
        let t = TestContext::new();

        let skill_dir =
            t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global).join("my-skill");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\n---\n");
        t.fs.add_file(skill_dir.join("tool.py"), "code");

        let ctx = t.ctx();
        uninstall("my-skill", ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();

        assert!(!t.fs.file_exists(&skill_dir.join("SKILL.md")), "skill dir should be removed");
    }

    #[test]
    fn uninstall_removes_lock_entry() {
        let t = TestContext::new();

        let agent_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        t.fs.add_file(agent_path.clone(), "# agent");

        // Write a lock file with an entry
        let mut packages = BTreeMap::new();
        packages.insert("my-agent".to_string(), sample_lock_entry());
        let lock = LockFile {
            version: 1,
            packages,
        };
        lockfile::save(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let result =
            uninstall("my-agent", ArtifactKind::Agent, InstallScope::Global, &ctx).unwrap();

        // Verify result fields
        assert_eq!(result.name, "my-agent");
        assert_eq!(result.kind, ArtifactKind::Agent);
        assert_eq!(result.scope, "global");
        assert!(result.was_tracked, "expected was_tracked to be true");

        let updated_lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!updated_lock.packages.contains_key("my-agent"), "lock entry should be removed");
    }

    #[test]
    fn uninstall_succeeds_even_without_lock_entry() {
        let t = TestContext::new();

        let agent_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("untracked.md");
        t.fs.add_file(agent_path, "# untracked agent");

        let ctx = t.ctx();
        let result = uninstall("untracked", ArtifactKind::Agent, InstallScope::Global, &ctx);
        assert!(result.is_ok(), "uninstall should succeed even without lock entry");

        let result = result.unwrap();
        assert!(!result.was_tracked, "expected was_tracked to be false for untracked artifact");
    }

    #[test]
    fn uninstall_with_delegates_to_perform_and_succeeds() {
        let t = TestContext::new();

        let agent_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        t.fs.add_file(agent_path, "# agent");

        let ctx = t.ctx();
        let result = uninstall("my-agent", ArtifactKind::Agent, InstallScope::Global, &ctx);
        assert!(result.is_ok(), "uninstall_with should succeed: {:?}", result.err());
    }

    // --- Platform-aware uninstall tests ---

    #[test]
    fn uninstall_cursor_removes_from_cursor_dir_and_per_platform_lock() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(chrono::Utc::now());
        let paths = test_paths_for(Platform::Cursor);

        // Install a file at the Cursor global path
        let agent_path =
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).join("my-agent.md");
        assert_eq!(
            agent_path,
            std::path::PathBuf::from("/home/testuser/.cursor/agents/my-agent.md")
        );
        fs.add_file(agent_path.clone(), "# agent");

        // Write lock entry into the Cursor lock file
        let mut packages = BTreeMap::new();
        packages.insert("my-agent".to_string(), sample_lock_entry());
        let lock = LockFile {
            version: 1,
            packages,
        };
        crate::lockfile::save(&lock, InstallScope::Global, &fs, &paths).unwrap();

        // Verify lock file path is Cursor-specific
        let lock_path = paths.lock_path(InstallScope::Global);
        assert!(
            lock_path.to_string_lossy().contains("cmx-lock-cursor.json"),
            "expected cursor-specific lock file, got: {}",
            lock_path.display()
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            uninstall("my-agent", ArtifactKind::Agent, InstallScope::Global, &ctx).unwrap();

        assert_eq!(result.name, "my-agent");
        assert!(result.was_tracked);
        assert!(!fs.file_exists(&agent_path), "agent file should be removed from cursor dir");

        let updated_lock = crate::lockfile::load(InstallScope::Global, &fs, &paths).unwrap();
        assert!(
            !updated_lock.packages.contains_key("my-agent"),
            "lock entry should be removed from cursor lock"
        );
    }

    #[test]
    fn uninstall_codex_agent_removes_toml_file() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(chrono::Utc::now());
        let paths = test_paths_for(Platform::Codex);

        let toml_path =
            paths.installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global);
        assert_eq!(
            toml_path,
            std::path::PathBuf::from("/home/testuser/.codex/agents/my-agent.toml")
        );
        fs.add_file(toml_path.clone(), "name = \"my-agent\"\n");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            uninstall("my-agent", ArtifactKind::Agent, InstallScope::Global, &ctx).unwrap();

        assert_eq!(result.name, "my-agent");
        assert!(!fs.file_exists(&toml_path), "codex agent TOML should be removed");
    }

    #[test]
    fn uninstall_many_removes_each_and_reports_not_found() {
        let t = TestContext::new();
        let skills_dir = t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global);
        for s in ["webapp-testing", "web-artifacts-builder"] {
            t.fs.add_file(skills_dir.join(s).join("SKILL.md"), "---\n---\n");
        }

        let ctx = t.ctx();
        let result = uninstall_many(
            &[
                "webapp-testing".to_string(),
                "web-artifacts-builder".to_string(),
                "nope".to_string(),
            ],
            ArtifactKind::Skill,
            InstallScope::Global,
            &ctx,
        )
        .unwrap();

        assert_eq!(result.removed.len(), 2, "both installed skills removed");
        assert_eq!(result.not_found, vec!["nope".to_string()], "missing one reported, not fatal");
        assert!(!t.fs.file_exists(&skills_dir.join("webapp-testing").join("SKILL.md")));
        assert!(!t.fs.file_exists(&skills_dir.join("web-artifacts-builder").join("SKILL.md")));
    }

    #[test]
    fn uninstall_removes_skill_tracked_for_another_platform() {
        // The slack-gif-creator case: a skill in the shared .agents/skills dir,
        // tracked for codex, must be removable with the DEFAULT (claude) context
        // — uninstall is cross-platform, not bound to the active platform.
        let t = TestContext::new(); // active platform = claude
        let codex = t.paths.with_platform(Platform::Codex);
        let skill_dir =
            codex.install_dir(ArtifactKind::Skill, InstallScope::Global).join("slack-gif");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\n---\n");
        let mut packages = BTreeMap::new();
        packages.insert("slack-gif".to_string(), sample_lock_entry());
        lockfile::save(
            &LockFile {
                version: 1,
                packages,
            },
            InstallScope::Global,
            &t.fs,
            &codex,
        )
        .unwrap();

        let ctx = t.ctx();
        let result =
            uninstall("slack-gif", ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();

        assert!(result.was_on_disk, "the shared .agents/skills copy was removed");
        assert!(result.platforms.contains(&Platform::Codex), "codex lock entry cleared");
        assert!(!t.fs.file_exists(&skill_dir.join("SKILL.md")), "physical copy gone");
        let codex_lock = lockfile::load(InstallScope::Global, &t.fs, &codex).unwrap();
        assert!(!codex_lock.packages.contains_key("slack-gif"), "codex lock entry removed");
    }
}
