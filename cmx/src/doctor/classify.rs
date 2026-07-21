//! Per-artifact classification for `cmx doctor`'s survey.
//!
//! Decides each installed artifact's [`ArtifactState`] from its content
//! checksum against the pre-loaded lock files and source availability built
//! by `locations.rs`, and assembles the raw per-location [`DoctorRow`]s the
//! survey later folds into logical artifacts (see `aggregate.rs`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::error::Result;
use crate::platform::Platform;
use crate::scan;
use crate::types::{ArtifactKind, InstallScope, LockFile};

use super::locations::LocationAgg;
use super::types::{ArtifactState, DoctorRow};

/// Classify one on-disk artifact against the lock files of every platform that
/// reads its location.
///
/// Tracked wins as soon as any platform's lock records the artifact with a
/// matching checksum; drifted means a lock entry exists but none matched. With
/// no lock entry, the artifact is *untracked* if a registered source provides it
/// (installed out-of-band → track via `install`) or *orphaned* if no source does
/// (hand-authored → adopt candidate).
/// Classify an installed artifact from its current content checksum.
///
/// Tracked when a lock entry's recorded checksum matches the current content;
/// drifted when a lock entry exists but the content has changed; untracked when
/// a registered source provides it but no lock records it; orphaned otherwise.
///
/// Pure: the caller hashes the artifact once (the same checksum feeds divergence
/// detection) and passes it here, so classification performs no I/O.
pub(crate) fn classify_installed(
    name: &str,
    agg: &LocationAgg,
    content_checksum: &str,
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    available_in_source: &HashMap<(ArtifactKind, String), Vec<String>>,
) -> ArtifactState {
    let mut found_entry = false;
    for &platform in &agg.platforms {
        let Some(lock) = locks.get(&(platform, agg.scope)) else {
            continue;
        };
        if let Some(entry) = lock.packages.get(name) {
            found_entry = true;
            if content_checksum == entry.installed_checksum {
                return ArtifactState::Tracked;
            }
        }
    }
    if found_entry {
        ArtifactState::Drifted
    } else if available_in_source.contains_key(&(agg.kind, name.to_string())) {
        ArtifactState::Untracked
    } else {
        ArtifactState::Orphaned
    }
}

/// The source an installed artifact came from, for the doctor `Source` column:
/// the lock entry's source repo when tracked/drifted, or the providing
/// source(s) when untracked (installed out-of-band).
pub(crate) fn source_of(
    name: &str,
    agg: &LocationAgg,
    state: ArtifactState,
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    available_in_source: &HashMap<(ArtifactKind, String), Vec<String>>,
) -> Option<String> {
    match state {
        ArtifactState::Tracked | ArtifactState::Drifted => agg.platforms.iter().find_map(|p| {
            locks
                .get(&(*p, agg.scope))
                .and_then(|l| l.packages.get(name))
                .map(|e| e.source.repo.clone())
        }),
        ArtifactState::Untracked => available_in_source
            .get(&(agg.kind, name.to_string()))
            .and_then(|sources| sources.first().cloned()),
        ArtifactState::Orphaned | ArtifactState::External => None,
    }
}

/// Read an installed artifact's declared version from its content file.
pub(crate) fn read_installed_version(
    kind: ArtifactKind,
    path: &Path,
    ctx: &AppContext<'_>,
) -> Option<String> {
    let content_path = kind.content_path(path);
    let content = ctx.fs.read_to_string(&content_path).ok()?;
    scan::extract_version_from_content(&content)
}

/// Build one [`DoctorRow`] per installed artifact across all locations.
pub(crate) fn build_rows(
    locations: &BTreeMap<PathBuf, LocationAgg>,
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    available: &HashMap<(ArtifactKind, String), Vec<String>>,
    external: &[String],
    ctx: &AppContext<'_>,
) -> Result<Vec<DoctorRow>> {
    let mut rows = Vec::new();
    for (dir, agg) in locations {
        if !ctx.fs.exists(dir) {
            continue;
        }
        // For skills the agent extension is irrelevant; for agents each location
        // maps to a single platform, so any attributed platform's view is correct.
        let pv = ctx.paths.with_platform(agg.platforms[0]);
        let names = config::installed_names(agg.kind, agg.scope, ctx.fs, &pv)?;
        for name in names {
            let path = pv.require_installed_artifact_path(agg.kind, &name, agg.scope)?;
            // Hash once: this checksum classifies the copy *and* decides content
            // divergence against the artifact's other copies.
            let content_checksum = checksum::checksum_artifact(&path, agg.kind, ctx.fs)?;
            let mut state = classify_installed(&name, agg, &content_checksum, locks, available);
            // An artifact cmx doesn't manage (orphaned/untracked) but that the
            // user has declared external is reclassified — managed by another
            // tool, not a cmx issue.
            if matches!(state, ArtifactState::Orphaned | ArtifactState::Untracked)
                && config::matches_external(external, &name, dir, &ctx.paths.home_dir)
            {
                state = ArtifactState::External;
            }
            let version = read_installed_version(agg.kind, &path, ctx);
            // The platforms cmx actually tracks this for: those whose lock file
            // records it (a subset of the location's readers).
            let tracked_for: Vec<Platform> = agg
                .platforms
                .iter()
                .copied()
                .filter(|p| {
                    locks.get(&(*p, agg.scope)).is_some_and(|l| l.packages.contains_key(&name))
                })
                .collect();
            let source = source_of(&name, agg, state, locks, available);
            rows.push(DoctorRow {
                kind: agg.kind,
                name,
                scope: agg.scope,
                location: dir.clone(),
                platforms: agg.platforms.clone(),
                tracked_for,
                state,
                version,
                source,
                content_checksum,
            });
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::platform::Platform;
    use crate::types::{ArtifactKind, InstallScope, LockEntry, LockFile, LockSource};

    use super::{LocationAgg, classify_installed, source_of};
    use crate::doctor::types::ArtifactState;

    fn make_lock(entries: &[(&str, &str, ArtifactKind)]) -> LockFile {
        let mut packages = std::collections::BTreeMap::new();
        for &(name, checksum, kind) in entries {
            packages.insert(
                name.to_string(),
                LockEntry {
                    artifact_type: kind,
                    version: None,
                    installed_at: "2024-01-01T00:00:00Z".to_string(),
                    source: LockSource {
                        repo: "test-source".to_string(),
                        path: format!("skills/{name}"),
                    },
                    source_checksum: checksum.to_string(),
                    installed_checksum: checksum.to_string(),
                },
            );
        }
        LockFile {
            version: 1,
            packages,
        }
    }

    fn simple_agg(kind: ArtifactKind, platforms: Vec<Platform>) -> LocationAgg {
        LocationAgg {
            kind,
            scope: InstallScope::Global,
            platforms,
        }
    }

    fn no_sources() -> HashMap<(ArtifactKind, String), Vec<String>> {
        HashMap::new()
    }

    fn source_provides(
        kind: ArtifactKind,
        name: &str,
    ) -> HashMap<(ArtifactKind, String), Vec<String>> {
        let mut m = HashMap::new();
        m.insert((kind, name.to_string()), vec!["test-source".to_string()]);
        m
    }

    // -----------------------------------------------------------------------
    // classify_installed
    // -----------------------------------------------------------------------

    #[test]
    fn classify_installed_tracked_when_lock_checksum_matches() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let mut locks = HashMap::new();
        locks.insert(
            (Platform::Claude, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:abc", ArtifactKind::Skill)]),
        );
        let result = classify_installed("my-skill", &agg, "sha256:abc", &locks, &no_sources());
        assert_eq!(result, ArtifactState::Tracked);
    }

    #[test]
    fn classify_installed_drifted_when_checksum_mismatches() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let mut locks = HashMap::new();
        locks.insert(
            (Platform::Claude, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:original", ArtifactKind::Skill)]),
        );
        let result = classify_installed("my-skill", &agg, "sha256:modified", &locks, &no_sources());
        assert_eq!(result, ArtifactState::Drifted);
    }

    #[test]
    fn classify_installed_tracked_when_second_platform_matches() {
        // Two platforms share a location; only the second one has a matching lock.
        // The function must return Tracked as soon as any platform matches.
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Codex, Platform::Claude]);
        let mut locks = HashMap::new();
        // Codex has a mismatched checksum
        locks.insert(
            (Platform::Codex, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:different", ArtifactKind::Skill)]),
        );
        // Claude has the matching checksum
        locks.insert(
            (Platform::Claude, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:abc", ArtifactKind::Skill)]),
        );
        let result = classify_installed("my-skill", &agg, "sha256:abc", &locks, &no_sources());
        assert_eq!(
            result,
            ArtifactState::Tracked,
            "Tracked must win when any platform's lock matches"
        );
    }

    #[test]
    fn classify_installed_untracked_when_no_lock_but_source_provides() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let locks = HashMap::new(); // no lock entries
        let available = source_provides(ArtifactKind::Skill, "my-skill");
        let result = classify_installed("my-skill", &agg, "sha256:abc", &locks, &available);
        assert_eq!(result, ArtifactState::Untracked);
    }

    #[test]
    fn classify_installed_orphaned_when_no_lock_and_no_source() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let locks = HashMap::new();
        let result = classify_installed("my-skill", &agg, "sha256:abc", &locks, &no_sources());
        assert_eq!(result, ArtifactState::Orphaned);
    }

    // -----------------------------------------------------------------------
    // source_of
    // -----------------------------------------------------------------------

    #[test]
    fn source_of_tracked_returns_lock_repo() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let mut locks = HashMap::new();
        locks.insert(
            (Platform::Claude, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:abc", ArtifactKind::Skill)]),
        );
        let result = source_of("my-skill", &agg, ArtifactState::Tracked, &locks, &no_sources());
        assert_eq!(result.as_deref(), Some("test-source"));
    }

    #[test]
    fn source_of_drifted_returns_lock_repo() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let mut locks = HashMap::new();
        locks.insert(
            (Platform::Claude, InstallScope::Global),
            make_lock(&[("my-skill", "sha256:abc", ArtifactKind::Skill)]),
        );
        let result = source_of("my-skill", &agg, ArtifactState::Drifted, &locks, &no_sources());
        assert_eq!(result.as_deref(), Some("test-source"));
    }

    #[test]
    fn source_of_untracked_returns_providing_source() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let available = source_provides(ArtifactKind::Skill, "my-skill");
        let result =
            source_of("my-skill", &agg, ArtifactState::Untracked, &HashMap::new(), &available);
        assert_eq!(result.as_deref(), Some("test-source"));
    }

    #[test]
    fn source_of_orphaned_returns_none() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let result =
            source_of("my-skill", &agg, ArtifactState::Orphaned, &HashMap::new(), &no_sources());
        assert!(result.is_none());
    }

    #[test]
    fn source_of_external_returns_none() {
        let agg = simple_agg(ArtifactKind::Skill, vec![Platform::Claude]);
        let result =
            source_of("my-skill", &agg, ArtifactState::External, &HashMap::new(), &no_sources());
        assert!(result.is_none());
    }
}
