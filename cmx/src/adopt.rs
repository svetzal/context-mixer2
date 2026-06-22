//! `cmx ... adopt` — bring orphaned, hand-authored artifacts under management.
//!
//! Adoption is the bridge from a disorganized pile (see [`crate::doctor`]) to a
//! managed set. It:
//!
//! 1. copies an orphaned artifact **verbatim** into the canonical home
//!    (`<config_dir>/home` by default), the tool-neutral source of truth;
//! 2. ensures the home is registered as a visible local source named `home`, so
//!    `install --all --platform <tool>` can project it outward with no further
//!    setup;
//! 3. records a lock entry (provenance `home`, with the artifact's checksum) for
//!    each platform that reads the orphan's location — so it reclassifies from
//!    *orphaned* to *tracked*.
//!
//! The original on-disk copy is **never moved or rewritten** — adoption only
//! copies and records provenance, so it is safe to run on a messy system.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::copy;
use crate::doctor::{self, ArtifactState, DoctorRow};
use crate::lockfile;
use crate::platform::Platform;
use crate::types::{
    self, ArtifactKind, InstallScope, LockEntry, LockSource, SourceEntry, SourceType,
};
use crate::uninstall;

/// The canonical source name under which the home is registered.
pub const HOME_SOURCE: &str = "home";

/// One adopted artifact.
#[derive(Debug)]
pub struct AdoptResult {
    pub kind: ArtifactKind,
    pub name: String,
    /// Where the canonical copy now lives in the home.
    pub home_path: PathBuf,
    /// Platforms whose lock files now track the original on-disk copy.
    pub platforms: Vec<Platform>,
}

/// The outcome of an adopt run.
#[derive(Debug)]
pub struct AdoptOutcome {
    pub adopted: Vec<AdoptResult>,
    pub home: PathBuf,
    /// Whether project (local) scope was surveyed for orphans.
    pub included_local: bool,
}

/// Resolve the effective canonical home directory from config.
pub(crate) fn resolve_home(ctx: &AppContext<'_>) -> Result<PathBuf> {
    let config = config::load_config(ctx.fs, ctx.paths)?;
    Ok(config::resolve_artifact_home(&config, ctx.paths))
}

/// Ensure the home directory exists and is registered as a local source named
/// `home`. Idempotent: re-registers (pointing at the resolved home) if absent or
/// stale, leaves it untouched otherwise.
pub(crate) fn ensure_home_source(home: &Path, ctx: &AppContext<'_>) -> Result<()> {
    ctx.fs.create_dir_all(home)?;
    let now = ctx.clock.now().to_rfc3339();
    config::mutate_sources(ctx.fs, ctx.paths, |sources| {
        let needs_write =
            sources.sources.get(HOME_SOURCE).is_none_or(|e| e.path.as_deref() != Some(home));
        if needs_write {
            sources.sources.insert(
                HOME_SOURCE.to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(home.to_path_buf()),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some(now),
                },
            );
        }
        Ok(())
    })
}

/// Copy one orphaned artifact into the home and record provenance for it on
/// every platform that reads its location.
fn adopt_row(row: &DoctorRow, home: &Path, ctx: &AppContext<'_>) -> Result<AdoptResult> {
    // The original on-disk artifact (verbatim source for the home copy).
    let representative = ctx.paths.with_platform(row.platforms[0]);
    let src = representative
        .installed_artifact_path(row.kind, &row.name, row.scope)
        .expect("installed_artifact_path: DoctorRow platforms support the artifact kind");

    // Destination in the home: <home>/<agents|skills>/<name[.md]>, copied
    // verbatim (no platform transform — the home always holds markdown).
    let dest_dir = home.join(row.kind.subdir_name());
    ctx.fs.create_dir_all(&dest_dir)?;
    let home_path = copy::copy_artifact_to(row.kind, &src, &dest_dir, ctx.fs)?;

    let cs = checksum::checksum_artifact(&home_path, row.kind, ctx.fs)?;
    let relative_path = types::relative_path_string(&home_path, home);
    let installed_at = ctx.clock.now().to_rfc3339();

    // Record provenance in each attributed platform's lock file so the original
    // reclassifies from orphaned to tracked. Verbatim copy ⇒ the home checksum
    // equals the on-disk checksum, so it lands as tracked, not drifted.
    for &platform in &row.platforms {
        let pv = ctx.paths.with_platform(platform);
        lockfile::mutate(row.scope, ctx.fs, &pv, |lock| {
            lock.packages.insert(
                row.name.clone(),
                LockEntry {
                    artifact_type: row.kind,
                    version: row.version.clone(),
                    installed_at: installed_at.clone(),
                    source: LockSource {
                        repo: HOME_SOURCE.to_string(),
                        path: relative_path.clone(),
                    },
                    source_checksum: cs.clone(),
                    installed_checksum: cs.clone(),
                },
            );
        })?;
    }

    Ok(AdoptResult {
        kind: row.kind,
        name: row.name.clone(),
        home_path,
        platforms: row.platforms.clone(),
    })
}

/// Adopt a set of orphan rows: ensure the home source is registered, then adopt
/// each. Non-orphan rows are ignored (callers pass orphans only).
fn adopt_rows(
    rows: &[DoctorRow],
    include_local: bool,
    ctx: &AppContext<'_>,
) -> Result<AdoptOutcome> {
    let home = resolve_home(ctx)?;
    ensure_home_source(&home, ctx)?;
    let mut adopted = Vec::new();
    for row in rows.iter().filter(|r| r.state == ArtifactState::Orphaned) {
        adopted.push(adopt_row(row, &home, ctx)?);
    }
    Ok(AdoptOutcome {
        adopted,
        home,
        included_local: include_local,
    })
}

/// Adopt every orphan the survey finds, optionally narrowed by artifact `kind`
/// and install `location` (a directory prefix). Backs `cmx doctor --adopt-all`
/// and `cmx <kind> adopt --all [--from <dir>]`.
///
/// Only *orphaned* artifacts are adopted — untracked (source-available) ones are
/// left for `install`. The `from` filter is how you exclude, say, a vendor
/// tool's bundled-skill directory while adopting your own.
pub fn adopt_all(
    kind: Option<ArtifactKind>,
    from: Option<&Path>,
    include_local: bool,
    ctx: &AppContext<'_>,
) -> Result<AdoptOutcome> {
    let report = doctor::survey(include_local, ctx)?;
    let rows: Vec<DoctorRow> = report
        .rows
        .into_iter()
        .filter(|r| r.state == ArtifactState::Orphaned)
        .filter(|r| kind.is_none_or(|k| r.kind == k))
        .filter(|r| from.is_none_or(|d| r.location.starts_with(d)))
        .collect();
    adopt_rows(&rows, include_local, ctx)
}

/// Adopt the named orphan artifacts of the given kind. Backs
/// `cmx {skill,agent} adopt <name>...`.
///
/// Each name is validated against the survey: a source-available (untracked)
/// artifact is steered to `install`; an already-tracked or drifted one is
/// rejected with an explanation; an unknown name errors. All-or-nothing — if any
/// name is invalid, nothing is adopted.
pub fn adopt_named(
    kind: ArtifactKind,
    names: &[String],
    include_local: bool,
    ctx: &AppContext<'_>,
) -> Result<AdoptOutcome> {
    let report = doctor::survey(include_local, ctx)?;
    let mut chosen = Vec::new();
    for name in names {
        let row = report.rows.iter().find(|r| r.kind == kind && &r.name == name);
        match row {
            Some(r) => match r.state {
                ArtifactState::Orphaned => chosen.push(r.clone()),
                ArtifactState::Untracked => anyhow::bail!(
                    "'{name}' is available in a registered source — run `cmx {kind} install {name}` to track it. \
                     (adopt is for hand-authored artifacts that no source provides.)"
                ),
                ArtifactState::Tracked => {
                    anyhow::bail!("'{name}' is already tracked — nothing to adopt.")
                }
                ArtifactState::Drifted => anyhow::bail!(
                    "'{name}' is tracked but locally modified (drifted), not orphaned — adopt does not yet \
                     re-home drifted artifacts. Inspect with `cmx info {name}`."
                ),
                ArtifactState::External => anyhow::bail!(
                    "'{name}' is marked external (managed by another tool) — remove it from the external \
                     list (`cmx config external remove ...`) before adopting it with cmx."
                ),
            },
            None => anyhow::bail!(
                "No {kind} named '{name}' found on disk. Run `cmx doctor` to see what is adoptable."
            ),
        }
    }
    adopt_rows(&chosen, include_local, ctx)
}

/// One unadopted artifact.
#[derive(Debug)]
pub struct UnadoptResult {
    pub kind: ArtifactKind,
    pub name: String,
    /// Whether the canonical copy was removed from the home.
    pub home_removed: bool,
    /// Platforms whose `home`-provenance lock entry was cleared (un-tracked).
    pub untracked_from: Vec<Platform>,
}

/// The outcome of an unadopt run.
#[derive(Debug)]
pub struct UnadoptOutcome {
    pub kind: ArtifactKind,
    pub unadopted: Vec<UnadoptResult>,
    /// Names that weren't adopted (not in the home, no `home` lock entry).
    pub not_adopted: Vec<String>,
}

/// Reverse [`adopt`](adopt_named) for one artifact: delete its canonical copy
/// from the home and clear every `home`-provenance lock entry for it (across all
/// platforms and scopes), un-tracking it. The on-disk originals/projections are
/// **left in place** — they simply revert to orphaned. Returns `Ok(None)` if the
/// artifact wasn't adopted (nothing in the home, no `home` lock entry).
fn unadopt_one(
    name: &str,
    kind: ArtifactKind,
    home: &Path,
    ctx: &AppContext<'_>,
) -> Result<Option<UnadoptResult>> {
    let home_path = kind.installed_path(name, &home.join(kind.subdir_name()));
    let home_present = ctx.fs.exists(&home_path);

    let mut untracked_from: Vec<Platform> = Vec::new();
    for platform in Platform::ALL {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        for scope in InstallScope::ALL {
            let tracked_from_home = lockfile::load(scope, ctx.fs, &pv)?
                .packages
                .get(name)
                .is_some_and(|e| e.source.repo == HOME_SOURCE);
            if tracked_from_home {
                lockfile::mutate(scope, ctx.fs, &pv, |lock| {
                    lock.packages.remove(name);
                })?;
                untracked_from.push(platform);
            }
        }
    }

    if !home_present && untracked_from.is_empty() {
        return Ok(None);
    }
    if home_present {
        uninstall::remove_installed(kind, &home_path, ctx.fs)?;
    }
    untracked_from.sort_by_key(|p| p.slug());
    untracked_from.dedup();
    Ok(Some(UnadoptResult {
        kind,
        name: name.to_string(),
        home_removed: home_present,
        untracked_from,
    }))
}

/// Unadopt several named artifacts. Best-effort: names that weren't adopted are
/// collected into `not_adopted` rather than aborting the batch. Backs
/// `cmx {skill,agent} unadopt <name>...`.
pub fn unadopt_many(
    names: &[String],
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<UnadoptOutcome> {
    let home = resolve_home(ctx)?;
    let mut unadopted = Vec::new();
    let mut not_adopted = Vec::new();
    for name in names {
        match unadopt_one(name, kind, &home, ctx)? {
            Some(r) => unadopted.push(r),
            None => not_adopted.push(name.clone()),
        }
    }
    Ok(UnadoptOutcome {
        kind,
        unadopted,
        not_adopted,
    })
}

/// Set up the canonical home without adopting anything: create the directory and
/// register it as the `home` source. Backs `cmx home init`.
pub fn home_init(ctx: &AppContext<'_>) -> Result<PathBuf> {
    let home = resolve_home(ctx)?;
    ensure_home_source(&home, ctx)?;
    Ok(home)
}

/// Return the resolved canonical home directory. Backs `cmx home path`.
pub fn home_path(ctx: &AppContext<'_>) -> Result<PathBuf> {
    resolve_home(ctx)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::filesystem::Filesystem;
    use crate::test_support::{TestContext, versioned_skill_content};
    use crate::types::InstallScope;

    /// Place an orphaned skill in a platform's install dir.
    fn place_orphan_skill(t: &TestContext, platform: Platform, name: &str, version: &str) {
        let pv = t.paths.with_platform(platform);
        let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
        t.fs.add_file(
            dir.join(name).join("SKILL.md"),
            versioned_skill_content("An orphaned skill", version),
        );
    }

    #[test]
    fn home_path_defaults_under_config_root() {
        let t = TestContext::new();
        let home = home_path(&t.ctx()).unwrap();
        assert_eq!(home, t.paths.config_dir.join("home"));
    }

    #[test]
    fn home_init_creates_dir_and_registers_source() {
        let t = TestContext::new();
        let home = home_init(&t.ctx()).unwrap();
        assert!(t.fs.exists(&home), "home dir created");

        let sources = config::load_sources(&t.fs, &t.paths).unwrap();
        let entry = sources.sources.get(HOME_SOURCE).expect("home source registered");
        assert!(matches!(entry.source_type, SourceType::Local));
        assert_eq!(entry.path.as_deref(), Some(home.as_path()));
    }

    #[test]
    fn adopt_copies_orphan_into_home_and_marks_tracked() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "my-skill", "1.0.0");

        // Before: doctor sees it as orphaned.
        let before = doctor::survey(false, &t.ctx()).unwrap();
        let row = before.rows.iter().find(|r| r.name == "my-skill").unwrap();
        assert_eq!(row.state, ArtifactState::Orphaned);

        let outcome = adopt_all(None, None, false, &t.ctx()).unwrap();
        assert_eq!(outcome.adopted.len(), 1);
        let adopted = &outcome.adopted[0];
        assert_eq!(adopted.name, "my-skill");

        // The canonical copy now lives in the home under skills/.
        let home_skill = outcome.home.join("skills").join("my-skill").join("SKILL.md");
        assert!(t.fs.exists(&home_skill), "skill copied into home at {}", home_skill.display());

        // After: doctor reclassifies the original as tracked.
        let after = doctor::survey(false, &t.ctx()).unwrap();
        let row = after.rows.iter().find(|r| r.name == "my-skill").unwrap();
        assert_eq!(row.state, ArtifactState::Tracked, "adopted orphan is now tracked");
        assert!(!after.has_issues(), "no issues remain after adopting the only orphan");
    }

    #[test]
    fn adopt_records_home_provenance_in_lockfile() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "my-skill", "2.3.4");
        adopt_all(None, None, false, &t.ctx()).unwrap();

        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let entry = lock.packages.get("my-skill").expect("lock entry written");
        assert_eq!(entry.source.repo, HOME_SOURCE);
        assert_eq!(entry.source.path, "skills/my-skill");
        assert_eq!(entry.version.as_deref(), Some("2.3.4"));
        assert_eq!(entry.source_checksum, entry.installed_checksum, "verbatim copy ⇒ equal");
    }

    #[test]
    fn adopt_does_not_move_the_original() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "my-skill", "1.0.0");
        let original = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("my-skill")
            .join("SKILL.md");
        adopt_all(None, None, false, &t.ctx()).unwrap();
        assert!(t.fs.exists(&original), "original copy is left in place, not moved");
    }

    #[test]
    fn adopt_named_adopts_only_the_named_orphan() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "keep", "1.0.0");
        place_orphan_skill(&t, Platform::Claude, "other", "1.0.0");

        let outcome =
            adopt_named(ArtifactKind::Skill, &["keep".to_string()], false, &t.ctx()).unwrap();
        assert_eq!(outcome.adopted.len(), 1);
        assert_eq!(outcome.adopted[0].name, "keep");

        // "other" remains orphaned.
        let report = doctor::survey(false, &t.ctx()).unwrap();
        let other = report.rows.iter().find(|r| r.name == "other").unwrap();
        assert_eq!(other.state, ArtifactState::Orphaned);
    }

    #[test]
    fn adopt_all_skips_source_available_untracked_artifacts() {
        let t = TestContext::new();
        // Source provides "vis-theory"; it's on disk with no lock (untracked).
        crate::test_support::setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "vis-theory",
            "1.0.0",
        );
        place_orphan_skill(&t, Platform::Claude, "vis-theory", "1.0.0");
        // A genuine orphan alongside it.
        place_orphan_skill(&t, Platform::Claude, "my-private", "1.0.0");

        let outcome = adopt_all(None, None, false, &t.ctx()).unwrap();
        let names: Vec<&str> = outcome.adopted.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"my-private"), "the true orphan is adopted");
        assert!(
            !names.contains(&"vis-theory"),
            "a source-available (untracked) artifact must NOT be adopted as private"
        );
    }

    #[test]
    fn adopt_named_steers_untracked_to_install() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "vis-theory",
            "1.0.0",
        );
        place_orphan_skill(&t, Platform::Claude, "vis-theory", "1.0.0");

        let err = adopt_named(ArtifactKind::Skill, &["vis-theory".to_string()], false, &t.ctx())
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("available in a registered source"), "got: {msg}");
        assert!(msg.contains("cmx skill install vis-theory"), "steers to install: {msg}");
    }

    #[test]
    fn adopt_all_from_filters_by_install_location() {
        let t = TestContext::new();
        // "mine" in ~/.claude/skills; "theirs" in the shared ~/.agents/skills.
        place_orphan_skill(&t, Platform::Claude, "mine", "1.0.0");
        place_orphan_skill(&t, Platform::Pi, "theirs", "1.0.0");

        let claude_skills = t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
        let outcome =
            adopt_all(Some(ArtifactKind::Skill), Some(&claude_skills), false, &t.ctx()).unwrap();
        let names: Vec<&str> = outcome.adopted.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["mine"], "only the orphan under the --from location is adopted");
    }

    #[test]
    fn adopt_named_adopts_multiple_in_one_call() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "alpha", "1.0.0");
        place_orphan_skill(&t, Platform::Claude, "beta", "1.0.0");
        place_orphan_skill(&t, Platform::Claude, "gamma", "1.0.0");

        let outcome = adopt_named(
            ArtifactKind::Skill,
            &["alpha".to_string(), "gamma".to_string()],
            false,
            &t.ctx(),
        )
        .unwrap();
        let mut names: Vec<&str> = outcome.adopted.iter().map(|a| a.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["alpha", "gamma"], "exactly the two named orphans");

        // beta remains orphaned.
        let report = doctor::survey(false, &t.ctx()).unwrap();
        assert_eq!(
            report.rows.iter().find(|r| r.name == "beta").unwrap().state,
            ArtifactState::Orphaned
        );
    }

    #[test]
    fn adopt_named_is_all_or_nothing_on_invalid_name() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "good", "1.0.0");

        // One valid orphan, one nonexistent → whole call fails, nothing adopted.
        let result = adopt_named(
            ArtifactKind::Skill,
            &["good".to_string(), "nope".to_string()],
            false,
            &t.ctx(),
        );
        assert!(result.is_err(), "invalid name should abort the batch");

        let report = doctor::survey(false, &t.ctx()).unwrap();
        assert_eq!(
            report.rows.iter().find(|r| r.name == "good").unwrap().state,
            ArtifactState::Orphaned,
            "the valid orphan must NOT have been adopted when the batch aborted"
        );
    }

    #[test]
    fn adopt_named_errors_when_no_matching_orphan() {
        let t = TestContext::new();
        let result = adopt_named(ArtifactKind::Skill, &["ghost".to_string()], false, &t.ctx());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No skill named 'ghost' found on disk"));
    }

    #[test]
    fn unadopt_reverses_adoption() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "tool-skill", "1.0.0");
        adopt_all(None, None, false, &t.ctx()).unwrap();

        // After adopt: in the home, and the original is tracked.
        let home = home_path(&t.ctx()).unwrap();
        assert!(t.fs.exists(&home.join("skills").join("tool-skill")), "adopted into home");

        let outcome =
            unadopt_many(&["tool-skill".to_string()], ArtifactKind::Skill, &t.ctx()).unwrap();
        assert_eq!(outcome.unadopted.len(), 1);
        assert!(outcome.unadopted[0].home_removed, "home copy removed");
        assert!(
            outcome.unadopted[0].untracked_from.contains(&Platform::Claude),
            "home-provenance lock entry cleared"
        );

        // Home copy gone; original on disk remains and is orphaned again.
        assert!(!t.fs.exists(&home.join("skills").join("tool-skill")), "removed from home");
        let original = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("tool-skill")
            .join("SKILL.md");
        assert!(t.fs.exists(&original), "tool-created original is left in place");
        let report = doctor::survey(false, &t.ctx()).unwrap();
        assert_eq!(
            report.rows.iter().find(|r| r.name == "tool-skill").unwrap().state,
            ArtifactState::Orphaned,
            "reverts to orphaned"
        );
    }

    #[test]
    fn unadopt_reports_not_adopted() {
        let t = TestContext::new();
        let outcome = unadopt_many(&["never".to_string()], ArtifactKind::Skill, &t.ctx()).unwrap();
        assert!(outcome.unadopted.is_empty());
        assert_eq!(outcome.not_adopted, vec!["never".to_string()]);
    }

    #[test]
    fn adopt_is_idempotent() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "my-skill", "1.0.0");
        adopt_all(None, None, false, &t.ctx()).unwrap();
        // Second run finds no orphans (the original is now tracked).
        let second = adopt_all(None, None, false, &t.ctx()).unwrap();
        assert!(second.adopted.is_empty(), "nothing left to adopt on the second run");
    }
}
