use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use anyhow::Result;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::scan;
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope, LockFile};

use super::types::{ArtifactState, DoctorArtifact, DoctorReport, DoctorRow, MissingRow};

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
) -> BTreeMap<PathBuf, LocationAgg> {
    let mut locations: BTreeMap<PathBuf, LocationAgg> = BTreeMap::new();
    for &platform in platforms {
        let pv = ctx.paths.with_platform(platform);
        for &scope in scopes {
            for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
                if !platform.supports(kind) {
                    continue;
                }
                let dir = pv.install_dir(kind, scope).expect("guarded by platform.supports(kind)");
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
    locations
}

/// Pre-load every `(platform, scope)` lock file once, so classification does no
/// repeated lock I/O.
fn load_all_locks(
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
fn classify_installed(
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

/// Map every `(kind, name)` available across registered sources to the source(s)
/// that provide it. Read-only — scans local source clones, never pulls.
fn available_in_sources(
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
fn read_installed_version(
    kind: ArtifactKind,
    path: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Option<String> {
    let content_path = kind.content_path(path);
    let content = ctx.fs.read_to_string(&content_path).ok()?;
    scan::extract_version_from_content(&content)
}

/// Severity ordering used to pick a logical artifact's consolidated state when
/// its copies disagree — the most actionable state wins.
pub(crate) fn state_severity(state: ArtifactState) -> u8 {
    match state {
        ArtifactState::Drifted => 4,
        ArtifactState::Orphaned => 3,
        ArtifactState::Untracked => 2,
        ArtifactState::External => 1,
        ArtifactState::Tracked => 0,
    }
}

/// Group per-location rows into logical artifacts — one per `(kind, name,
/// scope)`, listing every tool it's installed for. A skill installed to several
/// tools collapses to one artifact; it's flagged `diverged` only when its copies
/// actually disagree (different state or version), not merely for existing in
/// more than one place.
pub(crate) fn group_rows(rows: &[DoctorRow]) -> Vec<DoctorArtifact> {
    // Key by stringified kind so the map key is Ord without needing Ord on ArtifactKind.
    let mut groups: BTreeMap<(String, String, InstallScope), Vec<&DoctorRow>> = BTreeMap::new();
    for row in rows {
        groups
            .entry((row.kind.to_string(), row.name.clone(), row.scope))
            .or_default()
            .push(row);
    }

    groups
        .into_values()
        .map(|members| {
            let first = members[0];

            // Tools cmx manages this for: the union of each location's
            // tracked-for platforms (lockfile-backed), not every tool that reads
            // a shared directory.
            let mut tools: Vec<Platform> =
                members.iter().flat_map(|r| r.tracked_for.iter().copied()).collect();
            tools.sort_by_key(|p| p.slug());
            tools.dedup();

            let mut locations: Vec<PathBuf> = members.iter().map(|r| r.location.clone()).collect();
            locations.sort();
            locations.dedup();

            let versions: BTreeSet<Option<&str>> =
                members.iter().map(|r| r.version.as_deref()).collect();
            // Divergence is a content question: copies are diverged only when
            // their bytes actually differ. This catches genuinely different
            // copies that happen to share a version (or carry none), and stops
            // false-flagging byte-identical copies that merely differ in
            // tracking state (e.g. tracked for one tool, untracked for another).
            let checksums: BTreeSet<&str> =
                members.iter().map(|r| r.content_checksum.as_str()).collect();
            let diverged = checksums.len() > 1;

            // Consolidated state: the most actionable across copies.
            let state = members
                .iter()
                .map(|r| r.state)
                .max_by_key(|s| state_severity(*s))
                .unwrap_or(first.state);
            // Version only when all copies agree.
            let version = if versions.len() == 1 {
                first.version.clone()
            } else {
                None
            };
            // The distinct versions actually present, sorted — so the display can
            // name a skew (`3.2.0 / 3.3.0`) rather than collapsing to `-`.
            let mut distinct_versions: Vec<String> =
                versions.iter().filter_map(|v| v.map(str::to_string)).collect();
            distinct_versions.sort();

            // Source: the distinct provenance(s) across copies, joined when they
            // differ (rare — copies normally share a source).
            let mut sources: Vec<String> =
                members.iter().filter_map(|r| r.source.clone()).collect();
            sources.sort();
            sources.dedup();
            let source = if sources.is_empty() {
                None
            } else {
                Some(sources.join(", "))
            };

            DoctorArtifact {
                kind: first.kind,
                name: first.name.clone(),
                scope: first.scope,
                state,
                version,
                versions: distinct_versions,
                tools,
                source,
                locations,
                diverged,
            }
        })
        .collect()
}

/// Survey the whole system installation and classify every artifact.
///
/// Read-only: performs no writes. Surveys global scope always, and project
/// (local) scope when `include_local` is set.
pub fn survey(include_local: bool, ctx: &AppContext<'_>) -> Result<DoctorReport> {
    let scopes = survey_scopes(include_local);
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    // When the user has declared a managed set, `doctor` surveys only those
    // platforms; otherwise it inspects every supported platform.
    let platforms = if cfg.platforms.is_empty() {
        Platform::ALL.to_vec()
    } else {
        cfg.platforms.clone()
    };
    let locations = build_locations(ctx, &scopes, &platforms);
    let locks = load_all_locks(ctx, &scopes, &platforms)?;
    let available = available_in_sources(ctx)?;
    let external = cfg.external;

    let mut rows = Vec::new();
    for (dir, agg) in &locations {
        if !ctx.fs.exists(dir) {
            continue;
        }
        // For skills the agent extension is irrelevant; for agents each location
        // maps to a single platform, so any attributed platform's view is correct.
        let pv = ctx.paths.with_platform(agg.platforms[0]);
        let names = config::installed_names(agg.kind, agg.scope, ctx.fs, &pv)?;
        for name in names {
            let path = pv
                .installed_artifact_path(agg.kind, &name, agg.scope)
                .expect("guarded by platform.supports check in survey");
            // Hash once: this checksum classifies the copy *and* decides content
            // divergence against the artifact's other copies.
            let content_checksum = checksum::checksum_artifact(&path, agg.kind, ctx.fs)?;
            let mut state = classify_installed(&name, agg, &content_checksum, &locks, &available);
            // An artifact cmx doesn't manage (orphaned/untracked) but that the
            // user has declared external is reclassified — managed by another
            // tool, not a cmx issue.
            if matches!(state, ArtifactState::Orphaned | ArtifactState::Untracked)
                && config::matches_external(&external, &name, dir, &ctx.paths.home_dir)
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
            let source = source_of(&name, agg, state, &locks, &available);
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

    // Missing: lock entries whose artifact file is gone from disk.
    let mut missing = Vec::new();
    for ((platform, scope), lock) in &locks {
        let pv = ctx.paths.with_platform(*platform);
        for (name, entry) in &lock.packages {
            let kind = entry.artifact_type;
            if !pv.is_installed(kind, name, *scope, ctx.fs) {
                missing.push(MissingRow {
                    kind,
                    name: name.clone(),
                    scope: *scope,
                    platform: *platform,
                });
            }
        }
    }

    rows.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then(a.scope.cmp(&b.scope))
            .then(a.name.cmp(&b.name))
            .then(a.location.cmp(&b.location))
    });
    missing.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then(a.scope.cmp(&b.scope))
            .then(a.name.cmp(&b.name))
            .then(a.platform.slug().cmp(b.platform.slug()))
    });

    let artifacts = group_rows(&rows);

    Ok(DoctorReport {
        rows,
        artifacts,
        missing,
        included_local: include_local,
        surveyed_platforms: platforms.len(),
        scoped_to_managed: !cfg.platforms.is_empty(),
        show_all: false,
    })
}
