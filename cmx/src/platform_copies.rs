//! Shared primitive for iterating distinct physical artifact copies across platforms.
//!
//! Skills installed for multiple platforms may resolve to the **same physical
//! directory** (e.g. `.agents/skills` is shared by Codex, Pi, Opencode and
//! other skills-only tools). Rather than repeating the dedup-by-path loop in
//! every call site, this module provides a single primitive that:
//!
//! 1. Iterates the `candidates` list filtered by `platform.supports(kind)`.
//! 2. Resolves the artifact's install path for each platform.
//! 3. Skips platforms where the path does not exist on disk.
//! 4. Collapses platforms that share the same physical path into one entry.
//! 5. Calls `f(path, platforms)` once per distinct physical path, collecting
//!    the `Some(T)` values.
//!
//! Callers supply the closure that computes the per-copy result (checksum,
//! version, drift flag, etc.); the primitive handles only the iteration and
//! collapse.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::context::AppContext;
use crate::error::Result;
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

/// Iterate distinct physical copies of `name` across `candidates`, collapsing
/// platforms that share the same install directory into one entry, and call
/// `f(path, platforms) -> Result<Option<T>>` for each.
///
/// Returns the collected `T`s for every `Some(T)` the closure returned.
pub fn gather_platform_copies<F, T>(
    candidates: &[Platform],
    kind: ArtifactKind,
    name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
    mut f: F,
) -> Result<Vec<T>>
where
    F: FnMut(PathBuf, Vec<Platform>) -> Result<Option<T>>,
{
    let mut by_path: BTreeMap<PathBuf, Vec<Platform>> = BTreeMap::new();
    for &platform in candidates {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let Some(path) = pv.installed_artifact_path(kind, name, scope) else {
            continue;
        };
        if !ctx.fs.exists(&path) {
            continue;
        }
        by_path.entry(path).or_default().push(platform);
    }

    let mut results = Vec::new();
    for (path, platforms) in by_path {
        if let Some(t) = f(path, platforms)? {
            results.push(t);
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, skill_content};
    use crate::types::{ArtifactKind, CmxConfig, InstallScope};

    fn install_skill(
        fs: &crate::gateway::fakes::FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        platform: Platform,
        name: &str,
        content: &str,
        scope: InstallScope,
    ) {
        let dir = paths.with_platform(platform).install_dir(ArtifactKind::Skill, scope).unwrap();
        fs.add_file(dir.join(name).join("SKILL.md"), content);
    }

    fn save_managed(
        fs: &crate::gateway::fakes::FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        platforms: Vec<Platform>,
    ) {
        let config = CmxConfig {
            platforms,
            ..Default::default()
        };
        crate::config::save_config(&config, fs, paths).unwrap();
    }

    #[test]
    fn shared_dir_collapses_to_one_entry() {
        // Codex and Pi both resolve to .agents/skills at global scope.
        let t = TestContext::new();
        let content = skill_content("shared skill");
        install_skill(&t.fs, &t.paths, Platform::Codex, "my-skill", &content, InstallScope::Global);
        save_managed(&t.fs, &t.paths, vec![Platform::Codex, Platform::Pi]);

        let candidates = crate::config::managed_or_all_platforms(&t.fs, &t.paths).unwrap();
        let ctx = t.ctx();
        let entries = gather_platform_copies(
            &candidates,
            ArtifactKind::Skill,
            "my-skill",
            InstallScope::Global,
            &ctx,
            |path, platforms| Ok(Some((path, platforms))),
        )
        .unwrap();

        assert_eq!(entries.len(), 1, "shared dir must collapse to one entry");
        assert!(entries[0].1.contains(&Platform::Codex));
        assert!(entries[0].1.contains(&Platform::Pi));
    }

    #[test]
    fn two_distinct_dirs_yield_two_entries() {
        // Claude uses .claude/skills; Codex uses .agents/skills.
        let t = TestContext::new();
        let content = skill_content("multi-platform skill");
        install_skill(
            &t.fs,
            &t.paths,
            Platform::Claude,
            "my-skill",
            &content,
            InstallScope::Global,
        );
        install_skill(&t.fs, &t.paths, Platform::Codex, "my-skill", &content, InstallScope::Global);
        save_managed(&t.fs, &t.paths, vec![Platform::Claude, Platform::Codex]);

        let candidates = crate::config::managed_or_all_platforms(&t.fs, &t.paths).unwrap();
        let ctx = t.ctx();
        let entries = gather_platform_copies(
            &candidates,
            ArtifactKind::Skill,
            "my-skill",
            InstallScope::Global,
            &ctx,
            |path, platforms| Ok(Some((path, platforms))),
        )
        .unwrap();

        assert_eq!(entries.len(), 2, "distinct install dirs must yield two entries");
    }

    #[test]
    fn unsupported_platform_is_skipped() {
        // Pi supports only skills, not agents.  A request for agents must skip it.
        let t = TestContext::new();
        let candidates = vec![Platform::Pi];
        let ctx = t.ctx();
        let entries = gather_platform_copies(
            &candidates,
            ArtifactKind::Agent, // Pi does not support agents
            "my-agent",
            InstallScope::Global,
            &ctx,
            |path, platforms| Ok(Some((path, platforms))),
        )
        .unwrap();
        assert!(entries.is_empty(), "Pi must be skipped for agent kind");
    }

    #[test]
    fn missing_path_is_skipped() {
        let t = TestContext::new();
        // No files installed; the closure should never be called.
        let candidates = vec![Platform::Claude];
        let ctx = t.ctx();
        let entries = gather_platform_copies(
            &candidates,
            ArtifactKind::Skill,
            "missing-skill",
            InstallScope::Global,
            &ctx,
            |path, platforms| Ok(Some((path, platforms))),
        )
        .unwrap();
        assert!(entries.is_empty(), "missing path must be skipped");
    }
}
