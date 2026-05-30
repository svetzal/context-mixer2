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
use crate::doctor::{self, ArtifactState, DoctorRow};
use crate::lockfile;
use crate::platform::Platform;
use crate::types::{self, ArtifactKind, LockEntry, LockSource, SourceEntry, SourceType};

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
fn resolve_home(ctx: &AppContext<'_>) -> Result<PathBuf> {
    let config = config::load_config(ctx.fs, ctx.paths)?;
    Ok(config::resolve_artifact_home(&config, ctx.paths))
}

/// Ensure the home directory exists and is registered as a local source named
/// `home`. Idempotent: re-registers (pointing at the resolved home) if absent or
/// stale, leaves it untouched otherwise.
fn ensure_home_source(home: &Path, ctx: &AppContext<'_>) -> Result<()> {
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
    let src = representative.installed_artifact_path(row.kind, &row.name, row.scope);

    // Destination in the home: <home>/<agents|skills>/<name[.md]>, copied
    // verbatim (no platform transform — the home always holds markdown).
    let dest_dir = home.join(row.kind.subdir_name());
    ctx.fs.create_dir_all(&dest_dir)?;
    let home_path = row.kind.copy_to(&src, &dest_dir, ctx.fs)?;

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

/// Adopt every orphan the survey finds. Backs `cmx doctor --adopt-all`.
pub fn adopt_all(include_local: bool, ctx: &AppContext<'_>) -> Result<AdoptOutcome> {
    let report = doctor::survey(include_local, ctx)?;
    adopt_rows(&report.rows, include_local, ctx)
}

/// Adopt the orphan(s) of the given kind matching `name`. Backs
/// `cmx {skill,agent} adopt <name>`.
pub fn adopt_named(
    kind: ArtifactKind,
    name: &str,
    include_local: bool,
    ctx: &AppContext<'_>,
) -> Result<AdoptOutcome> {
    let report = doctor::survey(include_local, ctx)?;

    // If it's untracked (a registered source provides it), adopting it as
    // private would be wrong — steer to `install`, which records provenance.
    if report
        .rows
        .iter()
        .any(|r| r.kind == kind && r.name == name && r.state == ArtifactState::Untracked)
    {
        anyhow::bail!(
            "'{name}' is available in a registered source — run `cmx {kind} install {name}` to track it. \
             (adopt is for hand-authored artifacts that no source provides.)"
        );
    }

    let matching: Vec<DoctorRow> = report
        .rows
        .into_iter()
        .filter(|r| r.kind == kind && r.name == name && r.state == ArtifactState::Orphaned)
        .collect();
    if matching.is_empty() {
        anyhow::bail!(
            "No orphaned {kind} named '{name}' found. Run `cmx doctor` to see what is adoptable."
        );
    }
    adopt_rows(&matching, include_local, ctx)
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
        let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global);
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

        let outcome = adopt_all(false, &t.ctx()).unwrap();
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
        adopt_all(false, &t.ctx()).unwrap();

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
            .join("my-skill")
            .join("SKILL.md");
        adopt_all(false, &t.ctx()).unwrap();
        assert!(t.fs.exists(&original), "original copy is left in place, not moved");
    }

    #[test]
    fn adopt_named_adopts_only_the_named_orphan() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "keep", "1.0.0");
        place_orphan_skill(&t, Platform::Claude, "other", "1.0.0");

        let outcome = adopt_named(ArtifactKind::Skill, "keep", false, &t.ctx()).unwrap();
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

        let outcome = adopt_all(false, &t.ctx()).unwrap();
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

        let err = adopt_named(ArtifactKind::Skill, "vis-theory", false, &t.ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("available in a registered source"), "got: {msg}");
        assert!(msg.contains("cmx skill install vis-theory"), "steers to install: {msg}");
    }

    #[test]
    fn adopt_named_errors_when_no_matching_orphan() {
        let t = TestContext::new();
        let result = adopt_named(ArtifactKind::Skill, "ghost", false, &t.ctx());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No orphaned skill named 'ghost'"));
    }

    #[test]
    fn adopt_is_idempotent() {
        let t = TestContext::new();
        place_orphan_skill(&t, Platform::Claude, "my-skill", "1.0.0");
        adopt_all(false, &t.ctx()).unwrap();
        // Second run finds no orphans (the original is now tracked).
        let second = adopt_all(false, &t.ctx()).unwrap();
        assert!(second.adopted.is_empty(), "nothing left to adopt on the second run");
    }
}
