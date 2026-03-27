use anyhow::{Result, bail};

use crate::context::AppContext;
use crate::lockfile;
use crate::types::ArtifactKind;

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
// Public entry point
// ---------------------------------------------------------------------------

pub fn uninstall_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<UninstallResult> {
    perform_uninstall_with(name, kind, local, ctx)
}

// ---------------------------------------------------------------------------
// Perform (no println!)
// ---------------------------------------------------------------------------

pub(crate) fn perform_uninstall_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<UninstallResult> {
    let dir = ctx.paths.install_dir(kind, local);
    let target = kind.installed_path(name, &dir);

    if !ctx.fs.exists(&target) {
        let scope = if local { "local" } else { "global" };
        bail!("No {kind} named '{name}' found in {scope} scope.");
    }

    // Remove from disk
    match kind {
        ArtifactKind::Agent => {
            ctx.fs.remove_file(&target)?;
        }
        ArtifactKind::Skill => {
            ctx.fs.remove_dir_all(&target)?;
        }
    }

    // Remove from lock file
    let mut lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let was_tracked = lock.packages.remove(name).is_some();
    lockfile::save_with(&lock, local, ctx.fs, ctx.paths)?;

    let scope = if local { "local" } else { "global" };

    Ok(UninstallResult {
        name: name.to_string(),
        kind,
        scope,
        was_tracked,
    })
}

// ---------------------------------------------------------------------------
// Print (no business logic)
// ---------------------------------------------------------------------------

pub fn print_uninstall_result(result: &UninstallResult) {
    println!("Uninstalled {} ({}) from {} scope.", result.name, result.kind, result.scope);
    if !result.was_tracked {
        println!("  (no lock file entry found — artifact was untracked)");
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{make_ctx, sample_lock_entry, test_paths};
    use crate::types::{ArtifactKind, LockFile};
    use chrono::Utc;
    use std::collections::BTreeMap;

    #[test]
    fn uninstall_bails_when_agent_not_installed() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_ctx(&fs, &git, &clock, &paths);

        let result = perform_uninstall_with("nonexistent", ArtifactKind::Agent, false, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("nonexistent"), "unexpected: {msg}");
    }

    #[test]
    fn uninstall_removes_agent_file() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let agent_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        fs.add_file(agent_path.clone(), "# agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        perform_uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();

        assert!(!fs.file_exists(&agent_path), "agent file should be removed");
    }

    #[test]
    fn uninstall_removes_skill_dir() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let skill_dir = paths.install_dir(ArtifactKind::Skill, false).join("my-skill");
        fs.add_file(skill_dir.join("SKILL.md"), "---\n---\n");
        fs.add_file(skill_dir.join("tool.py"), "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        perform_uninstall_with("my-skill", ArtifactKind::Skill, false, &ctx).unwrap();

        assert!(!fs.file_exists(&skill_dir.join("SKILL.md")), "skill dir should be removed");
    }

    #[test]
    fn uninstall_removes_lock_entry() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let agent_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        fs.add_file(agent_path.clone(), "# agent");

        // Write a lock file with an entry
        let mut packages = BTreeMap::new();
        packages.insert("my-agent".to_string(), sample_lock_entry());
        let lock = LockFile {
            version: 1,
            packages,
        };
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = perform_uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();

        // Verify result fields
        assert_eq!(result.name, "my-agent");
        assert_eq!(result.kind, ArtifactKind::Agent);
        assert_eq!(result.scope, "global");
        assert!(result.was_tracked, "expected was_tracked to be true");

        let updated_lock = lockfile::load_with(false, &fs, &paths).unwrap();
        assert!(!updated_lock.packages.contains_key("my-agent"), "lock entry should be removed");
    }

    #[test]
    fn uninstall_succeeds_even_without_lock_entry() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let agent_path = paths.install_dir(ArtifactKind::Agent, false).join("untracked.md");
        fs.add_file(agent_path, "# untracked agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = perform_uninstall_with("untracked", ArtifactKind::Agent, false, &ctx);
        assert!(result.is_ok(), "uninstall should succeed even without lock entry");

        let result = result.unwrap();
        assert!(!result.was_tracked, "expected was_tracked to be false for untracked artifact");
    }

    #[test]
    fn uninstall_with_delegates_to_perform_and_succeeds() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let agent_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        fs.add_file(agent_path, "# agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = uninstall_with("my-agent", ArtifactKind::Agent, false, &ctx);
        assert!(result.is_ok(), "uninstall_with should succeed: {:?}", result.err());
    }
}
