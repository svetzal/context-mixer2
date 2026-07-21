//! Traversal and lock-loading for `cmx doctor`'s survey.
//!
//! Resolves the set of unique install locations across every platform/scope
//! combination, and pre-loads lock files and source-provided artifact names
//! once up front, so the classification stage (see `classify.rs`) performs no
//! repeated I/O.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope, LockFile};

/// The scopes to survey: global always, plus local when `include_local`.
pub(crate) fn survey_scopes(include_local: bool) -> Vec<InstallScope> {
    if include_local {
        vec![InstallScope::Global, InstallScope::Local]
    } else {
        vec![InstallScope::Global]
    }
}

/// Aggregated metadata for one unique install location.
pub(crate) struct LocationAgg {
    pub(crate) kind: ArtifactKind,
    pub(crate) scope: InstallScope,
    pub(crate) platforms: Vec<Platform>,
}

/// Build the set of unique install directories across every platform, attributing
/// each to the platforms that resolve to it. The shared `.agents/skills` cohort
/// collapses to a single location with many platforms.
pub(crate) fn build_locations(
    ctx: &AppContext<'_>,
    scopes: &[InstallScope],
    platforms: &[Platform],
) -> Result<BTreeMap<PathBuf, LocationAgg>> {
    let mut locations: BTreeMap<PathBuf, LocationAgg> = BTreeMap::new();
    for &platform in platforms {
        let pv = ctx.paths.with_platform(platform);
        for &scope in scopes {
            for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
                if !platform.supports(kind) {
                    continue;
                }
                let dir = pv.require_install_dir(kind, scope)?;
                locations
                    .entry(dir)
                    .or_insert_with(|| LocationAgg {
                        kind,
                        scope,
                        platforms: Vec::new(),
                    })
                    .platforms
                    .push(platform);
            }
        }
    }
    Ok(locations)
}

/// Pre-load every `(platform, scope)` lock file once, so classification does no
/// repeated lock I/O.
pub(crate) fn load_all_locks(
    ctx: &AppContext<'_>,
    scopes: &[InstallScope],
    platforms: &[Platform],
) -> Result<HashMap<(Platform, InstallScope), LockFile>> {
    let mut locks = HashMap::new();
    for &platform in platforms {
        let pv = ctx.paths.with_platform(platform);
        for &scope in scopes {
            locks.insert((platform, scope), lockfile::load(scope, ctx.fs, &pv)?);
        }
    }
    Ok(locks)
}

/// Map every `(kind, name)` available across registered sources to the source(s)
/// that provide it. Read-only — scans local source clones, never pulls.
pub(crate) fn available_in_sources(
    ctx: &AppContext<'_>,
) -> Result<HashMap<(ArtifactKind, String), Vec<String>>> {
    let mut map: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    for sa in source_iter::all_artifacts(ctx)? {
        map.entry((sa.artifact.kind, sa.artifact.name))
            .or_default()
            .push(sa.source_name);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::platform::Platform;
    use crate::test_support::TestContext;
    use crate::types::{ArtifactKind, InstallScope};

    use super::build_locations;

    #[test]
    fn build_locations_shared_dir_collapses_to_single_entry() {
        let t = TestContext::new();
        let ctx = t.ctx();
        // Codex and Pi both resolve .agents/skills for the skill kind — same physical dir
        let platforms = &[Platform::Codex, Platform::Pi];
        let scopes = &[InstallScope::Global];
        let locations = build_locations(&ctx, scopes, platforms).unwrap();
        // Both skill dirs should be at the same path — so they collapse into 1 entry
        // (Pi is skills-only, no agents; Codex has TOML agents at a different path)
        let skill_dirs: Vec<_> =
            locations.values().filter(|a| a.kind == ArtifactKind::Skill).collect();
        // There should be at least 1 unique skill dir for the shared cohort
        assert!(!skill_dirs.is_empty(), "shared skill dir must produce at least one location");
        // The shared dir must have both platforms in its list
        let shared = skill_dirs.iter().find(|a| a.platforms.len() > 1);
        assert!(shared.is_some(), "shared .agents/skills dir must list both platforms");
    }

    #[test]
    fn build_locations_skills_only_platform_has_no_agent_location() {
        let t = TestContext::new();
        let ctx = t.ctx();
        // Pi is skills-only
        let platforms = &[Platform::Pi];
        let scopes = &[InstallScope::Global];
        let locations = build_locations(&ctx, scopes, platforms).unwrap();
        // No agent locations should appear for Pi
        let has_agent = locations.values().any(|a| a.kind == ArtifactKind::Agent);
        assert!(!has_agent, "Pi does not support agents — no agent location expected");
    }
}
