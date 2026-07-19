use crate::error::Result;
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::platform::Platform;
use crate::platform_copies::gather_platform_copies;
use crate::types::{ArtifactKind, InstallScope};

use super::structural::ArtifactDiff;

/// A distinct physical install of the artifact, shared by ≥1 platform.
pub(super) struct InstalledCopy {
    pub(super) platforms: Vec<Platform>,
    pub(super) path: PathBuf,
    pub(super) checksum: String,
}

/// One installed copy with its computed comparison to the source.
pub(super) struct CopyEval {
    pub(super) copy: InstalledCopy,
    pub(super) matches: bool,
    pub(super) dir_diff: ArtifactDiff,
    pub(super) added: usize,
    pub(super) removed: usize,
}

/// Discover every installed copy of the artifact and the scope it lives at.
///
/// Skills can be installed on several platforms (some sharing the
/// `.agents/skills` directory), so they're surveyed across the managed
/// platforms. Agents are reformatted per platform (e.g. Codex TOML), so a
/// cross-platform byte comparison is meaningless — they stay single-copy on the
/// active platform.
pub(super) fn discover_copies(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(Vec<InstalledCopy>, InstallScope)> {
    if kind == ArtifactKind::Agent {
        return match config::find_installed_path(name, kind, ctx.fs, ctx.paths) {
            Some((path, scope)) => {
                let checksum = checksum::checksum_artifact(&path, kind, ctx.fs)?;
                Ok((
                    vec![InstalledCopy {
                        platforms: vec![ctx.paths.platform],
                        path,
                        checksum,
                    }],
                    scope,
                ))
            }
            None => Ok((Vec::new(), InstallScope::Global)),
        };
    }
    // Skills: global scope first, then project.
    for scope in InstallScope::ALL {
        let copies = gather_skill_copies(name, scope, ctx)?;
        if !copies.is_empty() {
            return Ok((copies, scope));
        }
    }
    Ok((Vec::new(), InstallScope::Global))
}

/// Gather distinct skill copies across the managed platforms at `scope`, one
/// entry per install directory (the shared `.agents/skills` dir collapses
/// several platforms into one copy).
fn gather_skill_copies(
    name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<InstalledCopy>> {
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    gather_platform_copies(&candidates, ArtifactKind::Skill, name, scope, ctx, |path, platforms| {
        let checksum = checksum::checksum_artifact(&path, ArtifactKind::Skill, ctx.fs)?;
        Ok(Some(InstalledCopy {
            platforms,
            path,
            checksum,
        }))
    })
}

/// Pick the platform to name in reconcile commands for a copy shared by several:
/// the active platform if it reads this copy, else a managed platform, else the
/// first — so `--from codex` is suggested over `--from opencode` for `promote`.
pub(super) fn representative_platform(
    copy: &InstalledCopy,
    active: Platform,
    managed: Option<&[Platform]>,
) -> Platform {
    if copy.platforms.contains(&active) {
        return active;
    }
    managed
        .and_then(|m| copy.platforms.iter().find(|p| m.contains(p)).copied())
        .or_else(|| copy.platforms.first().copied())
        .unwrap_or(active)
}

/// Compare each discovered copy to the source, computing the per-copy diff (and
/// its +/- totals) for the ones that differ.
pub(super) fn evaluate_copies(
    raw_copies: Vec<InstalledCopy>,
    kind: ArtifactKind,
    source_checksum: &str,
    source_path: &std::path::Path,
    source_name: &str,
    ctx: &AppContext<'_>,
) -> Result<Vec<CopyEval>> {
    use super::structural::diff_artifact;
    let mut evals = Vec::with_capacity(raw_copies.len());
    for copy in raw_copies {
        let matches = copy.checksum == source_checksum;
        let dir_diff = if matches {
            ArtifactDiff {
                changes: Vec::new(),
                unified: String::new(),
            }
        } else {
            diff_artifact(kind, &copy.path, source_path, source_name, ctx)?
        };
        let added = dir_diff.changes.iter().map(|c| c.added).sum();
        let removed = dir_diff.changes.iter().map(|c| c.removed).sum();
        evals.push(CopyEval {
            copy,
            matches,
            dir_diff,
            added,
            removed,
        });
    }
    Ok(evals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, agent_content, install_agent_on_disk, skill_content};
    use crate::types::{ArtifactKind, CmxConfig, InstallScope};

    // --- representative_platform ---

    fn make_copy(platforms: Vec<Platform>) -> InstalledCopy {
        InstalledCopy {
            platforms,
            path: std::path::PathBuf::from("/some/path"),
            checksum: "sha256:abc".to_string(),
        }
    }

    #[test]
    fn representative_platform_returns_active_when_present() {
        let copy = make_copy(vec![Platform::Claude, Platform::Codex]);
        assert_eq!(representative_platform(&copy, Platform::Claude, None), Platform::Claude);
    }

    #[test]
    fn representative_platform_prefers_managed_over_first_when_active_absent() {
        // Active (Gemini) not in platforms; managed includes Codex which is in platforms.
        let copy = make_copy(vec![Platform::Opencode, Platform::Codex]);
        let managed = vec![Platform::Codex];
        assert_eq!(
            representative_platform(&copy, Platform::Gemini, Some(&managed)),
            Platform::Codex
        );
    }

    #[test]
    fn representative_platform_falls_back_to_first_when_no_managed() {
        let copy = make_copy(vec![Platform::Opencode, Platform::Codex]);
        assert_eq!(representative_platform(&copy, Platform::Gemini, None), Platform::Opencode);
    }

    #[test]
    fn representative_platform_falls_back_to_active_when_platforms_empty() {
        let copy = make_copy(vec![]);
        assert_eq!(representative_platform(&copy, Platform::Claude, None), Platform::Claude);
    }

    // --- discover_copies: agent path ---

    #[test]
    fn discover_copies_agent_returns_single_copy_for_active_platform() {
        let t = TestContext::new();
        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        let ctx = t.ctx();
        let (copies, scope) = discover_copies("my-agent", ArtifactKind::Agent, &ctx).unwrap();

        assert_eq!(copies.len(), 1, "exactly one copy for an installed agent");
        assert_eq!(copies[0].platforms, vec![Platform::Claude], "active platform assigned");
        assert_eq!(scope, InstallScope::Global);
    }

    #[test]
    fn discover_copies_agent_returns_empty_when_not_installed() {
        let t = TestContext::new();

        let ctx = t.ctx();
        let (copies, scope) = discover_copies("my-agent", ArtifactKind::Agent, &ctx).unwrap();

        assert!(copies.is_empty(), "no copies when not installed");
        assert_eq!(scope, InstallScope::Global);
    }

    // --- discover_copies / gather_skill_copies: multi-platform ---

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
    fn discover_copies_skill_collapses_platforms_sharing_same_dir() {
        // Codex and Pi both resolve to .agents/skills at global scope.
        let t = TestContext::new();
        let content = skill_content("shared skill");
        install_skill(&t.fs, &t.paths, Platform::Codex, "my-skill", &content, InstallScope::Global);
        save_managed(&t.fs, &t.paths, vec![Platform::Codex, Platform::Pi]);

        let ctx = t.ctx();
        let (copies, scope) = discover_copies("my-skill", ArtifactKind::Skill, &ctx).unwrap();

        assert_eq!(copies.len(), 1, "shared dir collapses to one copy");
        assert_eq!(scope, InstallScope::Global);
        assert!(copies[0].platforms.contains(&Platform::Codex));
        assert!(copies[0].platforms.contains(&Platform::Pi));
    }

    #[test]
    fn discover_copies_skill_two_distinct_dirs_yields_two_copies() {
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

        let ctx = t.ctx();
        let (copies, _scope) = discover_copies("my-skill", ArtifactKind::Skill, &ctx).unwrap();

        assert_eq!(copies.len(), 2, "distinct install dirs → two copies");
    }

    #[test]
    fn discover_copies_skill_returns_local_scope_when_only_local_installed() {
        let t = TestContext::new();
        let content = skill_content("local skill");
        install_skill(&t.fs, &t.paths, Platform::Claude, "my-skill", &content, InstallScope::Local);
        save_managed(&t.fs, &t.paths, vec![Platform::Claude]);

        let ctx = t.ctx();
        let (copies, scope) = discover_copies("my-skill", ArtifactKind::Skill, &ctx).unwrap();

        assert_eq!(copies.len(), 1, "one copy at local scope");
        assert_eq!(
            scope,
            InstallScope::Local,
            "local scope returned when only local install exists"
        );
    }

    // --- evaluate_copies ---

    #[test]
    fn evaluate_copies_matching_checksum_returns_matches_true_and_empty_diff() {
        let t = TestContext::new();
        t.fs.add_file("/source/my-agent.md", "shared content\n");
        let source_path = std::path::Path::new("/source/my-agent.md");
        let source_checksum =
            checksum::checksum_artifact(source_path, ArtifactKind::Agent, &t.fs).unwrap();

        t.fs.add_file("/installed/my-agent.md", "shared content\n");
        let installed_path = std::path::PathBuf::from("/installed/my-agent.md");

        let copy = InstalledCopy {
            platforms: vec![Platform::Claude],
            path: installed_path,
            checksum: source_checksum.clone(),
        };

        let ctx = t.ctx();
        let evals = evaluate_copies(
            vec![copy],
            ArtifactKind::Agent,
            &source_checksum,
            source_path,
            "home",
            &ctx,
        )
        .unwrap();

        assert_eq!(evals.len(), 1);
        assert!(evals[0].matches, "identical checksum → matches");
        assert_eq!(evals[0].added, 0);
        assert_eq!(evals[0].removed, 0);
        assert!(evals[0].dir_diff.changes.is_empty());
    }

    #[test]
    fn evaluate_copies_differing_checksum_returns_matches_false_with_changes() {
        let t = TestContext::new();
        t.fs.add_file("/source/my-agent.md", "source line\n");
        let source_path = std::path::Path::new("/source/my-agent.md");
        let source_checksum =
            checksum::checksum_artifact(source_path, ArtifactKind::Agent, &t.fs).unwrap();

        t.fs.add_file("/installed/my-agent.md", "installed line\n");
        let installed_path = std::path::PathBuf::from("/installed/my-agent.md");
        let installed_checksum =
            checksum::checksum_artifact(&installed_path, ArtifactKind::Agent, &t.fs).unwrap();

        let copy = InstalledCopy {
            platforms: vec![Platform::Claude],
            path: installed_path,
            checksum: installed_checksum,
        };

        let ctx = t.ctx();
        let evals = evaluate_copies(
            vec![copy],
            ArtifactKind::Agent,
            &source_checksum,
            source_path,
            "home",
            &ctx,
        )
        .unwrap();

        assert_eq!(evals.len(), 1);
        assert!(!evals[0].matches, "different checksums → does not match");
        assert!(!evals[0].dir_diff.changes.is_empty(), "diff records changes");
        let total = evals[0].added + evals[0].removed;
        assert!(total > 0, "non-zero added+removed");
    }
}
