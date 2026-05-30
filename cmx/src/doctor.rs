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
    /// Present on disk with no lock entry on any platform that reads this
    /// location — e.g. a hand-authored artifact never installed via cmx.
    Orphaned,
}

impl ArtifactState {
    pub fn label(self) -> &'static str {
        match self {
            ArtifactState::Tracked => "tracked",
            ArtifactState::Drifted => "drifted",
            ArtifactState::Orphaned => "orphaned",
        }
    }
}

/// One installed artifact discovered on disk during the survey.
#[derive(Debug, Clone)]
pub struct DoctorRow {
    pub kind: ArtifactKind,
    pub name: String,
    pub scope: InstallScope,
    /// The resolved install directory the artifact was found in.
    pub location: PathBuf,
    /// Every platform that reads this location (more than one for the shared
    /// `.agents/skills` cohort).
    pub platforms: Vec<Platform>,
    pub state: ArtifactState,
    pub version: Option<String>,
    /// True when the same `(kind, name)` also appears in a *different* install
    /// location — genuine duplication, not the shared-directory cohort.
    pub duplicated: bool,
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
#[derive(Debug, Default)]
pub struct DoctorReport {
    pub rows: Vec<DoctorRow>,
    pub missing: Vec<MissingRow>,
    /// Whether project (local) scope was included in the survey.
    pub included_local: bool,
}

/// Per-state tallies for the summary line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateCounts {
    pub tracked: usize,
    pub drifted: usize,
    pub orphaned: usize,
    pub missing: usize,
    pub duplicated: usize,
}

impl DoctorReport {
    /// Tally rows by state for the summary line.
    pub fn counts(&self) -> StateCounts {
        let mut c = StateCounts {
            missing: self.missing.len(),
            ..StateCounts::default()
        };
        for row in &self.rows {
            match row.state {
                ArtifactState::Tracked => c.tracked += 1,
                ArtifactState::Drifted => c.drifted += 1,
                ArtifactState::Orphaned => c.orphaned += 1,
            }
            if row.duplicated {
                c.duplicated += 1;
            }
        }
        c
    }

    /// Whether the survey found anything that needs attention.
    ///
    /// Drift, orphans, and missing entries are issues. Cross-location
    /// duplication is reported but is *not* an issue on its own — projecting one
    /// curated set into many tools legitimately produces copies; only the states
    /// above represent unmanaged or broken state.
    pub fn has_issues(&self) -> bool {
        !self.missing.is_empty() || self.rows.iter().any(|r| r.state != ArtifactState::Tracked)
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
/// matching checksum; drifted means a lock entry exists but none matched;
/// orphaned means no platform's lock knows about it.
fn classify_installed(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    platforms: &[Platform],
    path: &std::path::Path,
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    ctx: &AppContext<'_>,
) -> Result<ArtifactState> {
    let mut found_entry = false;
    for &platform in platforms {
        let Some(lock) = locks.get(&(platform, scope)) else {
            continue;
        };
        if let Some(entry) = lock.packages.get(name) {
            found_entry = true;
            if !checksum::is_locally_modified(path, kind, entry, ctx.fs)? {
                return Ok(ArtifactState::Tracked);
            }
        }
    }
    Ok(if found_entry {
        ArtifactState::Drifted
    } else {
        ArtifactState::Orphaned
    })
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

/// Mark every row whose `(kind, name)` appears in more than one distinct install
/// location as duplicated.
fn mark_duplicates(rows: &mut [DoctorRow]) {
    let mut locations_by_artifact: HashMap<(ArtifactKind, String), BTreeSet<PathBuf>> =
        HashMap::new();
    for row in rows.iter() {
        locations_by_artifact
            .entry((row.kind, row.name.clone()))
            .or_default()
            .insert(row.location.clone());
    }
    for row in rows.iter_mut() {
        if let Some(locs) = locations_by_artifact.get(&(row.kind, row.name.clone())) {
            row.duplicated = locs.len() > 1;
        }
    }
}

/// Survey the whole system installation and classify every artifact.
///
/// Read-only: performs no writes. Surveys global scope always, and project
/// (local) scope when `include_local` is set.
pub fn survey(include_local: bool, ctx: &AppContext<'_>) -> Result<DoctorReport> {
    let scopes = survey_scopes(include_local);
    let locations = build_locations(ctx, &scopes);
    let locks = load_all_locks(ctx, &scopes)?;

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
            let state =
                classify_installed(&name, agg.kind, agg.scope, &agg.platforms, &path, &locks, ctx)?;
            let version = read_installed_version(agg.kind, &path, ctx);
            rows.push(DoctorRow {
                kind: agg.kind,
                name,
                scope: agg.scope,
                location: dir.clone(),
                platforms: agg.platforms.clone(),
                state,
                version,
                duplicated: false,
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

    mark_duplicates(&mut rows);

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

    Ok(DoctorReport {
        rows,
        missing,
        included_local: include_local,
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
    fn same_skill_in_two_locations_is_marked_duplicated() {
        let t = TestContext::new();
        // Same skill name in ~/.claude/skills and the shared ~/.agents/skills.
        install_skill(&t, Platform::Claude, "dup", "1.0.0", InstallScope::Global);
        install_skill(&t, Platform::Pi, "dup", "2.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let dup_rows: Vec<&DoctorRow> = report.rows.iter().filter(|r| r.name == "dup").collect();
        assert_eq!(dup_rows.len(), 2, "one row per distinct location");
        assert!(dup_rows.iter().all(|r| r.duplicated), "both rows flagged duplicated");
        assert_eq!(report.counts().duplicated, 2);
    }

    #[test]
    fn shared_cohort_skill_is_one_row_not_many() {
        let t = TestContext::new();
        // A single skill in the shared ~/.agents/skills dir, read by the whole
        // cohort, must be reported once — not once per platform.
        install_skill(&t, Platform::Pi, "shared", "1.0.0", InstallScope::Global);

        let report = survey(false, &t.ctx()).unwrap();
        let rows: Vec<&DoctorRow> = report.rows.iter().filter(|r| r.name == "shared").collect();
        assert_eq!(rows.len(), 1, "shared dir reported once");
        assert!(!rows[0].duplicated, "the cohort sharing one dir is not duplication");
        assert!(rows[0].platforms.len() > 1, "attributed to multiple cohort platforms");
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
