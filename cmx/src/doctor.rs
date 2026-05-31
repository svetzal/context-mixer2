//! `cmx doctor` — a read-only survey of the whole system installation.
//!
//! Doctor walks every platform's install directories (global, and project scope
//! when requested) and cross-references each per-platform lock file, then
//! classifies every artifact it finds. It is **read-only by contract**: it
//! mutates nothing and exists purely to make a disorganized installation
//! visible before any command changes a byte.
//!
//! ## Shared directories
//!
//! Several skills-only tools read the same physical `.agents/skills` directory.
//! Surveying naively per platform would report one on-disk skill many times.
//! Doctor instead keys the survey on the *resolved install directory*, scanning
//! each unique location once and attributing it to every platform that reads it.
//! An artifact is *tracked* if any attributed platform's lock file records it
//! with a matching checksum.

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

/// Classification of an installed artifact relative to the lock files that
/// should track it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactState {
    /// Present on disk and recorded in a lock file with a matching checksum.
    Tracked,
    /// Present on disk and in a lock file, but the on-disk copy was edited after
    /// install (checksum mismatch).
    Drifted,
    /// Present on disk with no lock entry, but a registered source provides an
    /// artifact of the same kind and name. Installed out-of-band — the fix is to
    /// track it via `install`, *not* adopt it as private.
    Untracked,
    /// Present on disk with no lock entry and **no** registered source provides
    /// it — a genuinely hand-authored artifact. The adopt candidate.
    Orphaned,
    /// Present on disk but declared external in config — managed by another tool,
    /// not cmx. Reported for visibility but never an issue.
    External,
}

impl ArtifactState {
    pub fn label(self) -> &'static str {
        match self {
            ArtifactState::Tracked => "tracked",
            ArtifactState::Drifted => "drifted",
            ArtifactState::Untracked => "untracked",
            ArtifactState::Orphaned => "orphaned",
            ArtifactState::External => "external",
        }
    }
}

/// One installed artifact discovered on disk during the survey, at a single
/// install location. This is the raw per-location unit; for the user-facing view
/// these are grouped into [`DoctorArtifact`] (one logical artifact across all the
/// tools it's installed for).
#[derive(Debug, Clone)]
pub struct DoctorRow {
    pub kind: ArtifactKind,
    pub name: String,
    pub scope: InstallScope,
    /// The resolved install directory the artifact was found in.
    pub location: PathBuf,
    /// Every platform that reads this location (more than one for the shared
    /// `.agents/skills` cohort). Used by adopt to record provenance.
    pub platforms: Vec<Platform>,
    /// The platforms whose lock file actually records this artifact — i.e. the
    /// tools cmx *manages* it for, a subset of `platforms`. Empty for artifacts
    /// with no lock entry (orphaned/untracked/external).
    pub tracked_for: Vec<Platform>,
    pub state: ArtifactState,
    pub version: Option<String>,
    /// The source this came from: the lock entry's repo when tracked/drifted, or
    /// the providing source when untracked. `None` for orphaned/external.
    pub source: Option<String>,
}

/// One *logical* artifact — a `(kind, name, scope)` grouped across every install
/// location cmx found it in. A skill projected to several tools is **one**
/// `DoctorArtifact` listing all those tools, not N "duplicates".
#[derive(Debug, Clone)]
pub struct DoctorArtifact {
    pub kind: ArtifactKind,
    pub name: String,
    pub scope: InstallScope,
    /// Consolidated state. When the copies disagree this is the most actionable
    /// one (see [`diverged`](Self::diverged)).
    pub state: ArtifactState,
    /// The version, when all copies agree; `None` if they differ or carry none.
    pub version: Option<String>,
    /// The distinct versions present across copies, sorted. One entry (or none)
    /// when copies agree; several when they diverge — lets the display name the
    /// skew (e.g. `3.2.0 / 3.3.0`) instead of an opaque `-`.
    pub versions: Vec<String>,
    /// The platforms cmx *manages* this artifact for (has a lock entry), unioned
    /// across its locations. Not every tool that merely reads a shared directory
    /// — only those cmx tracks it for. Empty when nothing tracks it.
    pub tools: Vec<Platform>,
    /// The source it came from (lock provenance), when all copies agree.
    pub source: Option<String>,
    /// The distinct install locations it occupies.
    pub locations: Vec<PathBuf>,
    /// True when the copies **disagree** — different state or different version
    /// across locations. This is the only multi-location situation worth
    /// flagging; consistent copies are just one skill installed to many tools.
    pub diverged: bool,
}

/// A lock entry whose artifact is no longer present on disk.
#[derive(Debug, Clone)]
pub struct MissingRow {
    pub kind: ArtifactKind,
    pub name: String,
    pub scope: InstallScope,
    pub platform: Platform,
}

/// The full read-only survey result.
///
/// `rows` is the raw per-location view (used by adopt and for detail);
/// `artifacts` is the grouped logical view (one entry per skill, listing the
/// tools it's installed for) used for display and counts.
#[derive(Debug, Default)]
pub struct DoctorReport {
    pub rows: Vec<DoctorRow>,
    pub artifacts: Vec<DoctorArtifact>,
    pub missing: Vec<MissingRow>,
    /// Whether project (local) scope was included in the survey.
    pub included_local: bool,
    /// Display hint: when `true`, the full inventory is shown; otherwise only
    /// artifacts that need attention (the default — `doctor` is for problems).
    pub show_all: bool,
}

impl DoctorReport {
    /// Whether a logical artifact needs attention — drifted/untracked/orphaned,
    /// or *any* artifact whose copies diverge across locations.
    ///
    /// A clean external or tracked artifact is fine: another tool managing it, or
    /// cmx managing it consistently, is the steady state. But a **divergence** —
    /// two copies at different versions or states — is a real anomaly worth
    /// surfacing whoever owns it; cmx just can't be the one to re-sync an external
    /// one (its owning tool must). So divergence is always a problem; only a
    /// *consistent* external/tracked artifact is healthy.
    pub fn is_problem(a: &DoctorArtifact) -> bool {
        match a.state {
            ArtifactState::External | ArtifactState::Tracked => a.diverged,
            _ => true,
        }
    }
}

/// Per-state tallies for the summary line. Counts are over *logical* artifacts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateCounts {
    pub tracked: usize,
    pub drifted: usize,
    pub untracked: usize,
    pub orphaned: usize,
    pub external: usize,
    pub missing: usize,
    /// Logical artifacts whose copies disagree across locations.
    pub diverged: usize,
}

impl DoctorReport {
    /// Tally logical artifacts by state for the summary line.
    pub fn counts(&self) -> StateCounts {
        let mut c = StateCounts {
            missing: self.missing.len(),
            ..StateCounts::default()
        };
        for a in &self.artifacts {
            match a.state {
                ArtifactState::Tracked => c.tracked += 1,
                ArtifactState::Drifted => c.drifted += 1,
                ArtifactState::Untracked => c.untracked += 1,
                ArtifactState::Orphaned => c.orphaned += 1,
                ArtifactState::External => c.external += 1,
            }
            // Every divergence counts — including external ones, which are a real
            // anomaly even if their owning tool (not cmx) must re-sync them.
            if a.diverged {
                c.diverged += 1;
            }
        }
        c
    }

    /// Whether the survey found anything that needs attention.
    ///
    /// Drift, untracked, orphaned, missing, and *diverged* (copies that
    /// disagree across locations) are issues. `tracked` and `external` (managed
    /// by another tool) are not — and a skill consistently installed to many
    /// tools is just that, not a problem.
    pub fn has_issues(&self) -> bool {
        !self.missing.is_empty() || self.artifacts.iter().any(Self::is_problem)
    }
}

/// The scopes to survey: global always, plus local when `include_local`.
fn survey_scopes(include_local: bool) -> Vec<InstallScope> {
    if include_local {
        vec![InstallScope::Global, InstallScope::Local]
    } else {
        vec![InstallScope::Global]
    }
}

/// Aggregated metadata for one unique install location.
struct LocationAgg {
    kind: ArtifactKind,
    scope: InstallScope,
    platforms: Vec<Platform>,
}

/// Build the set of unique install directories across every platform, attributing
/// each to the platforms that resolve to it. The shared `.agents/skills` cohort
/// collapses to a single location with many platforms.
fn build_locations(
    ctx: &AppContext<'_>,
    scopes: &[InstallScope],
) -> BTreeMap<PathBuf, LocationAgg> {
    let mut locations: BTreeMap<PathBuf, LocationAgg> = BTreeMap::new();
    for platform in Platform::ALL {
        let pv = ctx.paths.with_platform(platform);
        for &scope in scopes {
            for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
                if !platform.supports(kind) {
                    continue;
                }
                let dir = pv.install_dir(kind, scope);
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
) -> Result<HashMap<(Platform, InstallScope), LockFile>> {
    let mut locks = HashMap::new();
    for platform in Platform::ALL {
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
fn classify_installed(
    name: &str,
    agg: &LocationAgg,
    path: &std::path::Path,
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    available_in_source: &HashMap<(ArtifactKind, String), Vec<String>>,
    ctx: &AppContext<'_>,
) -> Result<ArtifactState> {
    let mut found_entry = false;
    for &platform in &agg.platforms {
        let Some(lock) = locks.get(&(platform, agg.scope)) else {
            continue;
        };
        if let Some(entry) = lock.packages.get(name) {
            found_entry = true;
            if !checksum::is_locally_modified(path, agg.kind, entry, ctx.fs)? {
                return Ok(ArtifactState::Tracked);
            }
        }
    }
    if found_entry {
        Ok(ArtifactState::Drifted)
    } else if available_in_source.contains_key(&(agg.kind, name.to_string())) {
        Ok(ArtifactState::Untracked)
    } else {
        Ok(ArtifactState::Orphaned)
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
fn source_of(
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
fn state_severity(state: ArtifactState) -> u8 {
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
fn group_rows(rows: &[DoctorRow]) -> Vec<DoctorArtifact> {
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

            let states: BTreeSet<&'static str> = members.iter().map(|r| r.state.label()).collect();
            let versions: BTreeSet<Option<&str>> =
                members.iter().map(|r| r.version.as_deref()).collect();
            let diverged = states.len() > 1 || versions.len() > 1;

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
    let locations = build_locations(ctx, &scopes);
    let locks = load_all_locks(ctx, &scopes)?;
    let available = available_in_sources(ctx)?;
    let external = config::load_config(ctx.fs, ctx.paths)?.external;

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
            let path = pv.installed_artifact_path(agg.kind, &name, agg.scope);
            let mut state = classify_installed(&name, agg, &path, &locks, &available, ctx)?;
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
            });
        }
    }

    // Missing: lock entries whose artifact file is gone from disk.
    let mut missing = Vec::new();
    for ((platform, scope), lock) in &locks {
        let pv = ctx.paths.with_platform(*platform);
        for (name, entry) in &lock.packages {
            let kind = entry.artifact_type;
            let path = pv.installed_artifact_path(kind, name, *scope);
            if !ctx.fs.exists(&path) {
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
        show_all: false,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        TestContext, make_lock_entry_with_checksum, save_lock_with_entry, versioned_skill_content,
    };
    use crate::types::InstallScope;

    /// Install a skill directory on disk for the given platform/scope and return
    /// its checksum so a lock entry can be made to match (or deliberately not).
    fn install_skill(
        t: &TestContext,
        platform: Platform,
        skill: &str,
        version: &str,
        scope: InstallScope,
    ) -> std::path::PathBuf {
        let pv = t.paths.with_platform(platform);
        let dir = pv.install_dir(ArtifactKind::Skill, scope);
        let skill_dir = dir.join(skill);
        t.fs.add_file(skill_dir.join("SKILL.md"), versioned_skill_content("A test skill", version));
        skill_dir
    }

    fn skill_checksum(t: &TestContext, skill_dir: &std::path::Path) -> String {
        crate::checksum::checksum_dir(skill_dir, &t.fs).unwrap()
    }

    /// Install a skill for `platform` and record a matching lock entry in that
    /// platform's lock file, so the survey classifies it `tracked` for that tool.
    fn track_skill(t: &TestContext, platform: Platform, skill: &str, version: &str) {
        let dir = install_skill(t, platform, skill, version, InstallScope::Global);
        let cs = skill_checksum(t, &dir);
        let entry =
            make_lock_entry_with_checksum(ArtifactKind::Skill, Some(version), "home", skill, &cs);
        let pv = t.paths.with_platform(platform);
        crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
            l.packages.insert(skill.to_string(), entry);
        })
        .unwrap();
    }

    // --- is_problem ---

    #[test]
    fn is_problem_matrix() {
        let art = |state, diverged| DoctorArtifact {
            kind: ArtifactKind::Skill,
            name: "x".to_string(),
            scope: InstallScope::Global,
            state,
            version: None,
            versions: vec![],
            tools: vec![],
            source: None,
            locations: vec![],
            diverged,
        };
        assert!(
            !DoctorReport::is_problem(&art(ArtifactState::Tracked, false)),
            "clean tracked: ok"
        );
        assert!(
            DoctorReport::is_problem(&art(ArtifactState::Tracked, true)),
            "tracked+diverged: problem"
        );
        assert!(
            DoctorReport::is_problem(&art(ArtifactState::Orphaned, false)),
            "orphaned: problem"
        );
        assert!(
            DoctorReport::is_problem(&art(ArtifactState::Untracked, false)),
            "untracked: problem"
        );
        assert!(
            DoctorReport::is_problem(&art(ArtifactState::Drifted, false)),
            "drifted: problem"
        );
        // A consistent external artifact is fine; a diverged one is an anomaly
        // worth surfacing even though its owning tool (not cmx) must re-sync it.
        assert!(
            !DoctorReport::is_problem(&art(ArtifactState::External, false)),
            "consistent external: ok"
        );
        assert!(
            DoctorReport::is_problem(&art(ArtifactState::External, true)),
            "external+diverged: surfaced as a problem"
        );
    }

    // --- ArtifactState::label ---

    #[test]
    fn artifact_state_labels() {
        assert_eq!(ArtifactState::Tracked.label(), "tracked");
        assert_eq!(ArtifactState::Drifted.label(), "drifted");
        assert_eq!(ArtifactState::Orphaned.label(), "orphaned");
    }

    // --- counts across mixed states ---

    #[test]
    fn counts_tally_tracked_and_drifted() {
        let t = TestContext::new();
        // One tracked (checksum matches lock), one drifted (lock checksum stale).
        let tracked_dir = install_skill(&t, Platform::Claude, "ok", "1.0.0", InstallScope::Global);
        let cs = skill_checksum(&t, &tracked_dir);
        install_skill(&t, Platform::Claude, "edited", "1.0.0", InstallScope::Global);
        // Both entries in one lock: "ok" matches its on-disk checksum, "edited" does not.
        crate::lockfile::mutate(InstallScope::Global, &t.fs, &t.paths, |lock| {
            lock.packages.insert(
                "ok".to_string(),
                make_lock_entry_with_checksum(
                    ArtifactKind::Skill,
                    Some("1.0.0"),
                    "home",
                    "ok",
                    &cs,
                ),
            );
            lock.packages.insert(
                "edited".to_string(),
                make_lock_entry_with_checksum(
                    ArtifactKind::Skill,
                    Some("1.0.0"),
                    "home",
                    "edited",
                    "sha256:stale",
                ),
            );
        })
        .unwrap();

        let report = survey(false, &t.ctx()).unwrap();
        let c = report.counts();
        assert_eq!(c.tracked, 1, "one tracked");
        assert_eq!(c.drifted, 1, "one drifted");
        assert_eq!(c.orphaned, 0);
    }

    // --- survey_scopes ---

    #[test]
    fn survey_scopes_global_only_by_default() {
        assert_eq!(survey_scopes(false), vec![InstallScope::Global]);
    }

    #[test]
    fn survey_scopes_includes_local_when_requested() {
        assert_eq!(survey_scopes(true), vec![InstallScope::Global, InstallScope::Local]);
    }

    // --- build_locations ---

    #[test]
    fn build_locations_collapses_shared_agents_skills_cohort() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let locations = build_locations(&ctx, &[InstallScope::Global]);

        // The shared global .agents/skills directory must be a single location
        // attributed to every cohort platform.
        let shared = t.paths.home_dir.join(".agents").join("skills");
        let agg = locations.get(&shared).expect("shared .agents/skills location present");
        assert_eq!(agg.kind, ArtifactKind::Skill);
        for p in [
            Platform::Opencode,
            Platform::Codex,
            Platform::Pi,
            Platform::Crush,
            Platform::Zed,
            Platform::Openhands,
        ] {
            assert!(agg.platforms.contains(&p), "{p} should read shared .agents/skills");
        }
    }

    // --- end-to-end survey classification ---

    #[test]
    fn orphaned_skill_in_claude_dir_is_reported() {
        let t = TestContext::new();
        // A hand-authored skill in ~/.claude/skills with no lock entry anywhere.
        install_skill(&t, Platform::Claude, "my-skill", "1.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "my-skill").expect("skill surveyed");
        assert_eq!(row.state, ArtifactState::Orphaned);
        assert_eq!(row.version.as_deref(), Some("1.0.0"));
        assert!(report.has_issues(), "an orphan is an issue");
    }

    #[test]
    fn untracked_when_on_disk_no_lock_but_source_provides_it() {
        let t = TestContext::new();
        // A registered source provides "vis-theory"...
        crate::test_support::setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "vis-theory",
            "1.0.0",
        );
        // ...and it's on disk with no lock entry (installed out-of-band).
        install_skill(&t, Platform::Claude, "vis-theory", "1.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "vis-theory").expect("surveyed");
        assert_eq!(
            row.state,
            ArtifactState::Untracked,
            "source-available + no lock → untracked, not orphaned"
        );
        assert_eq!(report.counts().untracked, 1);
        assert_eq!(report.counts().orphaned, 0);
        assert!(report.has_issues());
    }

    #[test]
    fn external_reclassifies_orphan_by_directory_rule() {
        let t = TestContext::new();
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        // A stock skill from another tool, in the Claude skills dir.
        install_skill(&t, Platform::Claude, "stock-skill", "1.0.0", InstallScope::Global);
        // Declare that whole directory external (home_dir is /home/testuser).
        let cfg = crate::types::CmxConfig {
            external: vec!["~/.claude/skills".to_string()],
            ..Default::default()
        };
        crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "stock-skill").expect("surveyed");
        assert_eq!(row.state, ArtifactState::External);
        assert_eq!(report.counts().external, 1);
        assert_eq!(report.counts().orphaned, 0);
        assert!(!report.has_issues(), "external artifacts are not issues");
    }

    #[test]
    fn external_reclassifies_orphan_by_name_rule() {
        let t = TestContext::new();
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        install_skill(&t, Platform::Claude, "apple", "1.0.0", InstallScope::Global);
        install_skill(&t, Platform::Claude, "mine", "1.0.0", InstallScope::Global);
        let cfg = crate::types::CmxConfig {
            external: vec!["apple".to_string()], // bare name
            ..Default::default()
        };
        crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let report = survey(false, &t.ctx()).unwrap();
        assert_eq!(
            report.rows.iter().find(|r| r.name == "apple").unwrap().state,
            ArtifactState::External
        );
        assert_eq!(
            report.rows.iter().find(|r| r.name == "mine").unwrap().state,
            ArtifactState::Orphaned,
            "a non-matching orphan stays orphaned"
        );
    }

    #[test]
    fn orphaned_only_when_no_source_provides_it() {
        let t = TestContext::new();
        // No source registered; a hand-authored skill on disk with no lock.
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        install_skill(&t, Platform::Claude, "my-private", "1.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "my-private").expect("surveyed");
        assert_eq!(row.state, ArtifactState::Orphaned);
        assert_eq!(report.counts().untracked, 0);
        assert_eq!(report.counts().orphaned, 1);
    }

    #[test]
    fn tracked_artifact_reports_its_lock_source() {
        let t = TestContext::new();
        // track_skill records provenance repo "home" in the lock entry.
        track_skill(&t, Platform::Claude, "mine", "1.0.0");

        let report = survey(false, &t.ctx()).unwrap();
        let art = report.artifacts.iter().find(|a| a.name == "mine").expect("grouped");
        assert_eq!(art.source.as_deref(), Some("home"), "source from the lock entry");
    }

    #[test]
    fn orphan_has_no_source() {
        let t = TestContext::new();
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        install_skill(&t, Platform::Claude, "loose", "1.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let art = report.artifacts.iter().find(|a| a.name == "loose").expect("grouped");
        assert!(art.source.is_none(), "an orphan has no source");
    }

    #[test]
    fn tracked_skill_matches_lock_checksum() {
        let t = TestContext::new();
        let skill_dir =
            install_skill(&t, Platform::Claude, "tracked", "1.0.0", InstallScope::Global);
        let cs = skill_checksum(&t, &skill_dir);
        let entry = make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            Some("1.0.0"),
            "home",
            "tracked",
            &cs,
        );
        save_lock_with_entry(&t.fs, &t.paths, "tracked", entry, InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "tracked").expect("skill surveyed");
        assert_eq!(row.state, ArtifactState::Tracked);
        assert!(!report.has_issues(), "a tracked artifact is not an issue");
    }

    #[test]
    fn drifted_skill_has_lock_entry_but_mismatched_checksum() {
        let t = TestContext::new();
        install_skill(&t, Platform::Claude, "drifted", "1.0.0", InstallScope::Global);
        let entry = make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            Some("1.0.0"),
            "home",
            "drifted",
            "sha256:stale_checksum_from_install_time",
        );
        save_lock_with_entry(&t.fs, &t.paths, "drifted", entry, InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let row = report.rows.iter().find(|r| r.name == "drifted").expect("skill surveyed");
        assert_eq!(row.state, ArtifactState::Drifted);
        assert!(report.has_issues());
    }

    #[test]
    fn missing_skill_in_lock_but_not_on_disk() {
        let t = TestContext::new();
        let entry = make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            Some("1.0.0"),
            "home",
            "ghost",
            "sha256:whatever",
        );
        save_lock_with_entry(&t.fs, &t.paths, "ghost", entry, InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        assert!(report.rows.is_empty(), "nothing on disk");
        let m = report
            .missing
            .iter()
            .find(|m| m.name == "ghost")
            .expect("missing entry reported");
        assert_eq!(m.kind, ArtifactKind::Skill);
        assert_eq!(m.platform, Platform::Claude);
        assert!(report.has_issues());
    }

    #[test]
    fn same_skill_in_two_tools_is_one_artifact_not_duplicated() {
        let t = TestContext::new();
        // Same skill, same version, tracked for claude (~/.claude/skills) and pi
        // (~/.agents/skills) — one logical artifact managed for both tools.
        track_skill(&t, Platform::Claude, "multi", "1.0.0");
        track_skill(&t, Platform::Pi, "multi", "1.0.0");

        let report = survey(false, &t.ctx()).unwrap();
        let arts: Vec<&DoctorArtifact> =
            report.artifacts.iter().filter(|a| a.name == "multi").collect();
        assert_eq!(arts.len(), 1, "one logical artifact, not two duplicates");
        assert_eq!(arts[0].state, ArtifactState::Tracked);
        assert!(!arts[0].diverged, "identical copies do not diverge");
        // Tools = the platforms cmx tracks it for (lockfile-backed), not every
        // cohort tool that merely reads .agents/skills.
        assert!(arts[0].tools.contains(&Platform::Claude));
        assert!(arts[0].tools.contains(&Platform::Pi));
        assert!(
            !arts[0].tools.contains(&Platform::Crush),
            "crush reads .agents/skills but isn't tracked for it — must not be listed"
        );
        // The raw per-location rows still exist (two locations) for adopt/detail.
        assert_eq!(report.rows.iter().filter(|r| r.name == "multi").count(), 2);
    }

    #[test]
    fn same_skill_at_different_versions_is_diverged() {
        let t = TestContext::new();
        install_skill(&t, Platform::Claude, "skew", "1.0.0", InstallScope::Global);
        install_skill(&t, Platform::Pi, "skew", "2.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let art = report.artifacts.iter().find(|a| a.name == "skew").expect("grouped");
        assert!(art.diverged, "different versions across locations should diverge");
        assert!(art.version.is_none(), "no single agreed version");
        assert_eq!(report.counts().diverged, 1);
        assert!(report.has_issues(), "divergence is an issue");
    }

    #[test]
    fn shared_cohort_skill_lists_only_tools_it_is_tracked_for() {
        let t = TestContext::new();
        // One skill in the shared ~/.agents/skills dir, tracked for pi and codex
        // (both wrote lock entries). It's one artifact whose Tools lists exactly
        // those two — not the other cohort tools that merely read the directory.
        track_skill(&t, Platform::Pi, "shared", "1.0.0");
        track_skill(&t, Platform::Codex, "shared", "1.0.0");

        let report = survey(false, &t.ctx()).unwrap();
        let arts: Vec<&DoctorArtifact> =
            report.artifacts.iter().filter(|a| a.name == "shared").collect();
        assert_eq!(arts.len(), 1, "shared dir reported once");
        assert!(!arts[0].diverged, "consistent copies don't diverge");
        assert!(arts[0].tools.contains(&Platform::Pi));
        assert!(arts[0].tools.contains(&Platform::Codex));
        assert!(
            !arts[0].tools.contains(&Platform::Crush) && !arts[0].tools.contains(&Platform::Zed),
            "cohort readers without a lock entry are not listed as tracked-for tools"
        );
    }

    #[test]
    fn empty_system_has_no_issues() {
        let t = TestContext::new();
        let report = survey(false, &t.ctx()).unwrap();
        assert!(report.rows.is_empty());
        assert!(report.missing.is_empty());
        assert!(!report.has_issues());
        assert_eq!(report.counts(), StateCounts::default());
    }

    #[test]
    fn counts_tally_each_state() {
        let t = TestContext::new();
        install_skill(&t, Platform::Claude, "orphan-a", "1.0.0", InstallScope::Global);
        install_skill(&t, Platform::Claude, "orphan-b", "1.0.0", InstallScope::Global);
        let report = survey(false, &t.ctx()).unwrap();
        let c = report.counts();
        assert_eq!(c.orphaned, 2);
        assert_eq!(c.tracked, 0);
        assert_eq!(c.drifted, 0);
    }
}
