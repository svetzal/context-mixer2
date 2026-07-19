//! Shared primitive for removing an artifact's physical copies and lock entries
//! across multiple platforms.
//!
//! Both `cmx/src/uninstall.rs` and `cmx-core/src/skill_install/remove.rs` share
//! the same two-phase pattern: (1) collect distinct physical paths and clear
//! per-platform lock entries, then (2) delete each distinct path once.  This
//! module extracts that shared core so the pattern does not drift between callers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Summary of what [`remove_artifact_across_platforms`] did.
pub struct RemoveArtifactResult {
    /// Distinct physical paths that were removed from disk.  Empty when the
    /// artifact was not on disk (lock-entry-only reconciliation).
    pub removed_paths: Vec<PathBuf>,
    /// Platforms whose lock entry for the artifact was cleared.
    pub platforms_cleared: Vec<Platform>,
    /// `true` when at least one physical path was removed.
    pub was_on_disk: bool,
    /// `true` when at least one lock entry was removed.
    pub was_tracked: bool,
}

// ---------------------------------------------------------------------------
// Core primitive
// ---------------------------------------------------------------------------

/// Remove all physical copies of `name` across `candidates` (filtered to those
/// that support `kind`) and clear each platform's lock entry for it.
///
/// **Shared-directory dedup:** platforms sharing the same physical install
/// directory (e.g. `.agents/skills` for Codex + Pi) resolve to the same path;
/// the path is collected once and deleted once.  Lock entries are cleared
/// per-platform regardless of disk presence, so stale entries (file already
/// gone) are also reconciled.
pub fn remove_artifact_across_platforms(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    candidates: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<RemoveArtifactResult> {
    let mut paths_to_delete: BTreeSet<PathBuf> = BTreeSet::new();
    let mut platforms_cleared: Vec<Platform> = Vec::new();
    let mut was_tracked = false;

    for &platform in candidates {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);

        if let Some(path) = pv.installed_artifact_path(kind, name, scope) {
            if ctx.fs.exists(&path) {
                paths_to_delete.insert(path);
            }
        }

        let lock = lockfile::load(scope, ctx.fs, &pv)?;
        if lock.packages.contains_key(name) {
            lockfile::mutate(scope, ctx.fs, &pv, |l| {
                l.packages.remove(name);
            })?;
            platforms_cleared.push(platform);
            was_tracked = true;
        }
    }

    let was_on_disk = !paths_to_delete.is_empty();
    let removed_paths: Vec<PathBuf> = paths_to_delete.into_iter().collect();
    for path in &removed_paths {
        match kind {
            ArtifactKind::Agent => ctx.fs.remove_file(path)?,
            ArtifactKind::Skill => ctx.fs.remove_dir_all(path)?,
        }
    }

    Ok(RemoveArtifactResult {
        removed_paths,
        platforms_cleared,
        was_on_disk,
        was_tracked,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::lockfile;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, make_lock_entry_with_checksum};
    use crate::types::{ArtifactKind, CmxConfig, InstallScope, LockFile};
    use std::collections::BTreeMap;

    fn seed_lock(t: &TestContext, platform: Platform, name: &str) {
        let pv = t.paths.with_platform(platform);
        let mut packages = BTreeMap::new();
        packages.insert(
            name.to_string(),
            make_lock_entry_with_checksum(
                ArtifactKind::Skill,
                Some("1.0.0"),
                "home",
                &format!("skills/{name}"),
                "sha256:abc",
            ),
        );
        lockfile::save(
            &LockFile {
                version: 1,
                packages,
            },
            InstallScope::Global,
            &t.fs,
            &pv,
        )
        .unwrap();
    }

    fn add_skill(t: &TestContext, platform: Platform, name: &str) {
        let pv = t.paths.with_platform(platform);
        let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
        t.fs.add_file(dir.join(name).join("SKILL.md"), "---\n---\n");
    }

    #[test]
    fn removes_skill_dir_and_clears_lock_entry() {
        let t = TestContext::new();
        add_skill(&t, Platform::Claude, "my-skill");
        seed_lock(&t, Platform::Claude, "my-skill");

        let ctx = t.ctx();
        let candidates = vec![Platform::Claude];
        let r = remove_artifact_across_platforms(
            "my-skill",
            ArtifactKind::Skill,
            InstallScope::Global,
            &candidates,
            &ctx,
        )
        .unwrap();

        assert!(r.was_on_disk);
        assert!(r.was_tracked);
        assert_eq!(r.platforms_cleared, vec![Platform::Claude]);
        let skill_md = t
            .paths
            .with_platform(Platform::Claude)
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("my-skill")
            .join("SKILL.md");
        assert!(!t.fs.file_exists(&skill_md), "skill SKILL.md should be gone");
    }

    #[test]
    fn shared_dir_deleted_once_both_locks_cleared() {
        // Codex and Pi share .agents/skills; one file removal, two lock clears.
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Codex, Platform::Pi],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        add_skill(&t, Platform::Codex, "my-skill"); // adds to .agents/skills/my-skill
        seed_lock(&t, Platform::Codex, "my-skill");
        seed_lock(&t, Platform::Pi, "my-skill");

        let ctx = t.ctx();
        let candidates = vec![Platform::Codex, Platform::Pi];
        let r = remove_artifact_across_platforms(
            "my-skill",
            ArtifactKind::Skill,
            InstallScope::Global,
            &candidates,
            &ctx,
        )
        .unwrap();

        assert!(r.was_on_disk);
        assert_eq!(r.removed_paths.len(), 1, "only one physical dir deleted");
        assert!(r.platforms_cleared.contains(&Platform::Codex));
        assert!(r.platforms_cleared.contains(&Platform::Pi));
    }

    #[test]
    fn unsupported_platform_skipped() {
        // Pi doesn't support agents.
        let t = TestContext::new();
        let ctx = t.ctx();
        let candidates = vec![Platform::Pi];
        let r = remove_artifact_across_platforms(
            "my-agent",
            ArtifactKind::Agent,
            InstallScope::Global,
            &candidates,
            &ctx,
        )
        .unwrap();
        assert!(!r.was_on_disk);
        assert!(!r.was_tracked);
        assert!(r.platforms_cleared.is_empty());
    }

    #[test]
    fn stale_lock_entry_cleared_even_when_not_on_disk() {
        let t = TestContext::new();
        // Only lock entry, no file on disk.
        seed_lock(&t, Platform::Claude, "ghost");
        let ctx = t.ctx();
        let candidates = vec![Platform::Claude];
        let r = remove_artifact_across_platforms(
            "ghost",
            ArtifactKind::Skill,
            InstallScope::Global,
            &candidates,
            &ctx,
        )
        .unwrap();
        assert!(!r.was_on_disk, "no file on disk");
        assert!(r.was_tracked, "but lock entry was cleared");
        assert!(r.platforms_cleared.contains(&Platform::Claude));
    }
}
