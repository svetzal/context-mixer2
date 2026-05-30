use anyhow::{Result, bail};

use crate::context::AppContext;
use crate::lockfile;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct UninstallResult {
    pub name: String,
    pub kind: ArtifactKind,
    pub scope: &'static str,
    pub was_tracked: bool,
    /// Whether the artifact was present on disk. `false` means we only
    /// reconciled a stale lock entry (the file was already gone) — the case
    /// `cmx doctor` reports as "missing".
    pub was_on_disk: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn uninstall(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<UninstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let target = ctx.paths.installed_artifact_path(kind, name, scope);
    let on_disk = ctx.fs.exists(&target);
    let tracked = lockfile::load(scope, ctx.fs, ctx.paths)?.packages.contains_key(name);

    // Nothing to do only when the artifact is neither on disk nor in the lock
    // file. A tracked-but-absent artifact (what `cmx doctor` reports as
    // "missing") is still reconcilable: we drop the stale lock entry below.
    if !on_disk && !tracked {
        bail!(
            "No {kind} named '{name}' found in {} scope (not on disk, no lock entry).",
            scope.label()
        );
    }

    // Remove from disk if present (absent is fine — we're reconciling).
    if on_disk {
        kind.remove_installed(&target, ctx.fs)?;
    }

    // Remove the lock entry if there is one.
    let was_tracked = if tracked {
        lockfile::mutate(scope, ctx.fs, ctx.paths, |lock| lock.packages.remove(name).is_some())?
    } else {
        false
    };

    Ok(UninstallResult {
        name: name.to_string(),
        kind,
        scope: scope.label(),
        was_tracked,
        was_on_disk: on_disk,
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
    fn uninstall_pi_agent_is_rejected() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(chrono::Utc::now());
        let paths = test_paths_for(Platform::Pi);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = uninstall("whatever", ArtifactKind::Agent, InstallScope::Global, &ctx);
        assert!(result.is_err(), "pi must reject agent uninstall");
        assert!(result.unwrap_err().to_string().contains("pi"));
    }
}
