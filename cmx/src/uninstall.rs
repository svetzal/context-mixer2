use anyhow::{Result, bail};

use crate::context::AppContext;
use crate::lockfile;
use crate::types::{ArtifactKind, scope_label};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub struct UninstallResult {
    pub name: String,
    pub kind: ArtifactKind,
    pub scope: &'static str,
    pub was_tracked: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn uninstall_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<UninstallResult> {
    let dir = ctx.paths.install_dir(kind, local);
    let target = kind.installed_path(name, &dir);

    if !ctx.fs.exists(&target) {
        let scope = scope_label(local);
        bail!("No {kind} named '{name}' found in {scope} scope.");
    }

    // Remove from disk
    kind.remove_installed(&target, ctx.fs)?;

    // Remove from lock file
    let was_tracked = lockfile::mutate_with(local, ctx.fs, ctx.paths, |lock| {
        lock.packages.remove(name).is_some()
    })?;

    let scope = scope_label(local);

    Ok(UninstallResult {
        name: name.to_string(),
        kind,
        scope,
        was_tracked,
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
    use crate::types::{ArtifactKind, LockFile};
    use std::collections::BTreeMap;

    #[test]
    fn uninstall_bails_when_agent_not_installed() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let result = uninstall_with("nonexistent", ArtifactKind::Agent, false, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("nonexistent"), "unexpected: {msg}");
    }

    #[test]
    fn uninstall_removes_agent_file() {
        let t = TestContext::new();

        let agent_path = t.paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        t.fs.add_file(agent_path.clone(), "# agent");

        let ctx = t.ctx();
        uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();

        assert!(!t.fs.file_exists(&agent_path), "agent file should be removed");
    }

    #[test]
    fn uninstall_removes_skill_dir() {
        let t = TestContext::new();

        let skill_dir = t.paths.install_dir(ArtifactKind::Skill, false).join("my-skill");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\n---\n");
        t.fs.add_file(skill_dir.join("tool.py"), "code");

        let ctx = t.ctx();
        uninstall_with("my-skill", ArtifactKind::Skill, false, &ctx).unwrap();

        assert!(!t.fs.file_exists(&skill_dir.join("SKILL.md")), "skill dir should be removed");
    }

    #[test]
    fn uninstall_removes_lock_entry() {
        let t = TestContext::new();

        let agent_path = t.paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        t.fs.add_file(agent_path.clone(), "# agent");

        // Write a lock file with an entry
        let mut packages = BTreeMap::new();
        packages.insert("my-agent".to_string(), sample_lock_entry());
        let lock = LockFile {
            version: 1,
            packages,
        };
        lockfile::save_with(&lock, false, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let result = uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();

        // Verify result fields
        assert_eq!(result.name, "my-agent");
        assert_eq!(result.kind, ArtifactKind::Agent);
        assert_eq!(result.scope, "global");
        assert!(result.was_tracked, "expected was_tracked to be true");

        let updated_lock = lockfile::load_with(false, &t.fs, &t.paths).unwrap();
        assert!(!updated_lock.packages.contains_key("my-agent"), "lock entry should be removed");
    }

    #[test]
    fn uninstall_succeeds_even_without_lock_entry() {
        let t = TestContext::new();

        let agent_path = t.paths.install_dir(ArtifactKind::Agent, false).join("untracked.md");
        t.fs.add_file(agent_path, "# untracked agent");

        let ctx = t.ctx();
        let result = uninstall_with("untracked", ArtifactKind::Agent, false, &ctx);
        assert!(result.is_ok(), "uninstall should succeed even without lock entry");

        let result = result.unwrap();
        assert!(!result.was_tracked, "expected was_tracked to be false for untracked artifact");
    }

    #[test]
    fn uninstall_with_delegates_to_perform_and_succeeds() {
        let t = TestContext::new();

        let agent_path = t.paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        t.fs.add_file(agent_path, "# agent");

        let ctx = t.ctx();
        let result = uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx);
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
        let agent_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
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
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        // Verify lock file path is Cursor-specific
        let lock_path = paths.lock_path(false);
        assert!(
            lock_path.to_string_lossy().contains("cmx-lock-cursor.json"),
            "expected cursor-specific lock file, got: {}",
            lock_path.display()
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();

        assert_eq!(result.name, "my-agent");
        assert!(result.was_tracked);
        assert!(!fs.file_exists(&agent_path), "agent file should be removed from cursor dir");

        let updated_lock = crate::lockfile::load_with(false, &fs, &paths).unwrap();
        assert!(
            !updated_lock.packages.contains_key("my-agent"),
            "lock entry should be removed from cursor lock"
        );
    }
}
