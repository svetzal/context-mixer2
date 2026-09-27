//! `cmx agent list` / `cmx skill list` / `cmx list`.

use crate::error::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::context::AppContext;
use crate::doctor::{self, ArtifactState};
use crate::flags::SurveyScope;
use crate::outdated;
use crate::source_iter::{self, SourceArtifactInfo};
use crate::table::Table;
use crate::types::{ArtifactKind, InstallScope, LockEntry};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// How an installed artifact's version compares to what its source currently offers.
pub enum ListStatus {
    /// Installed version matches the source's current version.
    Ok,
    /// The source offers a newer version than what's installed.
    Outdated,
    /// The source provides no version for this artifact.
    Unversioned,
    /// No registered source currently provides this artifact.
    SourceMissing,
    /// The source has marked this artifact deprecated.
    Deprecated,
}

impl ListStatus {
    /// The lowercase label used in list's human-readable and JSON output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Outdated => "outdated",
            Self::Unversioned => "unversioned",
            Self::SourceMissing => "source missing",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AvailableVersion {
    Version(String),
    Unversioned,
    SourceMissing,
}

/// One row in the listing — a logical artifact (grouped across the platforms
/// it's installed for, via [`crate::doctor`]).
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    /// The artifact's name.
    pub name: String,
    /// The version currently installed, if tracked.
    pub installed_version: Option<String>,
    /// The version currently offered by the source, if any.
    pub available_version: Option<String>,
    /// The source it came from (repo name only, no path).
    pub source: Option<String>,
    /// The platforms cmx tracks it for.
    pub platforms: Vec<String>,
    /// How the installed version compares to what the source offers.
    pub status: ListStatus,
}

/// Listing rows for a single artifact kind, grouped by install scope — the
/// result of `cmx agent list` / `cmx skill list`.
#[derive(Clone, Debug, Serialize)]
pub struct ListKindOutput {
    /// Whether these rows are agents or skills.
    pub kind: ArtifactKind,
    /// Rows grouped by install scope (global/local).
    pub rows: BTreeMap<InstallScope, Vec<Row>>,
}

/// Listing rows for both artifact kinds, grouped by install scope — the
/// result of `cmx list`.
#[derive(Clone, Debug, Serialize)]
pub struct ListOutput {
    /// Agent rows grouped by install scope (global/local).
    pub agents: BTreeMap<InstallScope, Vec<Row>>,
    /// Skill rows grouped by install scope (global/local).
    pub skills: BTreeMap<InstallScope, Vec<Row>>,
}

pub(crate) fn table_str(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    Table {
        headers: vec![
            "Name",
            "Installed",
            "Available",
            "Source",
            "Platforms",
            "Status",
        ],
        padded_cols: 6,
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    crate::display::util::version_label(r.installed_version.as_deref()).to_string(),
                    display_available_version(r.available_version.as_deref(), r.status),
                    display_source(r.source.as_deref()),
                    display_platforms(&r.platforms),
                    r.status.label().to_string(),
                ]
            })
            .collect(),
    }
    .render()
}

pub(crate) fn display_platforms(platforms: &[String]) -> String {
    if platforms.is_empty() {
        "none".to_string()
    } else {
        platforms.join(", ")
    }
}

pub(crate) fn section_str(label: &str, rows: &[Row]) -> String {
    let mut out = format!("{label}:\n");
    if rows.is_empty() {
        // Deliberately not `table::empty_state`: the section label/colon is
        // already written above, so this is a short indented in-section
        // placeholder, not a standalone empty-state message.
        out.push_str("  (none)\n");
    } else {
        out.push_str(&table_str(rows));
    }
    out.push('\n');
    out
}

fn display_available_version(version: Option<&str>, status: ListStatus) -> String {
    match version {
        Some(version) => version.to_string(),
        None if status == ListStatus::SourceMissing => "source missing".to_string(),
        None => crate::display::util::version_label(None).to_string(),
    }
}

fn display_source(source: Option<&str>) -> String {
    source.unwrap_or("no source").to_string()
}

/// Decide `Ok` vs `Outdated` (and the other status arms) for one artifact.
///
/// `Ok` is a **content** claim, not just a version-string match: it is
/// decided per tracked lock entry via [`outdated::behind_source`], the same
/// checksum-aware decision `cmx outdated` makes, so a source artifact edited
/// without a version bump is reported `Outdated` here too.
///
/// `tracked` holds the lock entry of every platform tracking the artifact at
/// its scope; the artifact is `Outdated` when **any** of them is behind. The
/// entries are judged one by one, never through an aggregate, because
/// per-platform baselines may legitimately disagree (one platform's stale
/// `source_checksum` over content that is current is not staleness). With no
/// tracked entry the artifact is untracked, which reads as `Outdated`.
/// `current_source_checksum` is the source's checksum right now.
fn list_status(
    available: &AvailableVersion,
    deprecated: bool,
    tracked: &[&LockEntry],
    current_source_checksum: Option<&str>,
) -> ListStatus {
    if deprecated {
        return ListStatus::Deprecated;
    }

    let available_version = match available {
        AvailableVersion::SourceMissing => return ListStatus::SourceMissing,
        AvailableVersion::Unversioned => return ListStatus::Unversioned,
        AvailableVersion::Version(v) => v,
    };

    let Some(current_checksum) = current_source_checksum else {
        // No checksum available for the current source offering — fall back
        // to outdated, matching the prior behavior for any mismatch.
        return ListStatus::Outdated;
    };

    let behind = |entry: Option<&LockEntry>| {
        outdated::behind_source(entry, current_checksum, Some(available_version))
    };
    let is_behind = if tracked.is_empty() {
        behind(None)
    } else {
        tracked.iter().any(|&entry| behind(Some(entry)))
    };
    if is_behind {
        ListStatus::Outdated
    } else {
        ListStatus::Ok
    }
}

/// List installed artifacts of a single kind, grouped by install scope.
/// Backs `cmx agent list` / `cmx skill list`.
pub fn list_kind(
    kind: ArtifactKind,
    include_external: bool,
    ctx: &AppContext<'_>,
) -> Result<ListKindOutput> {
    Ok(ListKindOutput {
        kind,
        rows: rows_by_scope(kind, include_external, ctx)?,
    })
}

/// List installed agents and skills, grouped by install scope. Backs `cmx list`.
pub fn list_all(include_external: bool, ctx: &AppContext<'_>) -> Result<ListOutput> {
    Ok(ListOutput {
        agents: rows_by_scope(ArtifactKind::Agent, include_external, ctx)?,
        skills: rows_by_scope(ArtifactKind::Skill, include_external, ctx)?,
    })
}

/// Build list rows for `kind` from the cross-platform [`doctor`] survey — one row
/// per logical artifact, with the platforms it's tracked for and an
/// available-version comparison drawn from the registered sources.
///
/// By default `list` is the cmx-managed inventory and omits artifacts declared
/// external (another tool owns them); pass `include_external` to show them too.
fn rows_by_scope(
    kind: ArtifactKind,
    include_external: bool,
    ctx: &AppContext<'_>,
) -> Result<BTreeMap<InstallScope, Vec<Row>>> {
    let report = doctor::survey(SurveyScope::GlobalAndLocal, ctx)?;
    let source_versions = source_iter::all_with_checksums(ctx)?;
    let locks = doctor::load_all_locks(
        ctx,
        &InstallScope::ALL,
        &crate::config::managed_or_all_platforms(ctx.fs, ctx.paths)?,
    )?;

    let mut by_scope: BTreeMap<InstallScope, Vec<Row>> = BTreeMap::new();
    for a in report
        .artifacts
        .iter()
        .filter(|a| a.kind == kind && (include_external || a.state != ArtifactState::External))
    {
        let infos = source_versions.get(&a.name);
        let available = available_version(infos, a.source.as_deref());
        let preferred = preferred_source_info(infos, a.source.as_deref());
        let deprecated = preferred.is_some_and(|i| i.deprecated);
        let current_source_checksum = preferred.map(|i| i.checksum.as_str());
        let tracked: Vec<&LockEntry> = a
            .tools
            .iter()
            .filter_map(|&platform| locks.get(&(platform, a.scope)))
            .filter_map(|lock| lock.packages.get(&a.name))
            .filter(|entry| entry.artifact_type == a.kind)
            .collect();

        by_scope.entry(a.scope).or_default().push(Row {
            name: a.name.clone(),
            installed_version: a.version.clone(),
            available_version: match &available {
                AvailableVersion::Version(version) => Some(version.clone()),
                AvailableVersion::Unversioned | AvailableVersion::SourceMissing => None,
            },
            source: a.source.clone(),
            platforms: a.tools.iter().map(ToString::to_string).collect(),
            status: list_status(&available, deprecated, &tracked, current_source_checksum),
        });
    }
    Ok(by_scope)
}

fn preferred_source_info<'a>(
    infos: Option<&'a Vec<SourceArtifactInfo>>,
    from: Option<&str>,
) -> Option<&'a SourceArtifactInfo> {
    let infos = infos?;
    infos
        .iter()
        .find(|i| from.is_some_and(|f| i.source_name == f))
        .or_else(|| infos.first())
}

/// The version a source offers for an artifact: prefer the source it was
/// installed from (`from`), else the first source that provides it.
fn available_version(
    infos: Option<&Vec<SourceArtifactInfo>>,
    from: Option<&str>,
) -> AvailableVersion {
    match preferred_source_info(infos, from) {
        Some(info) => match &info.version {
            Some(version) => AvailableVersion::Version(version.clone()),
            None => AvailableVersion::Unversioned,
        },
        None => AvailableVersion::SourceMissing,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, setup_source, versioned_skill_content};

    fn make_row(name: &str) -> Row {
        Row {
            name: name.to_string(),
            installed_version: Some("1.0.0".to_string()),
            available_version: Some("1.0.0".to_string()),
            source: Some("guidelines".to_string()),
            platforms: vec!["claude".to_string()],
            status: ListStatus::Ok,
        }
    }

    // --- section_str / table_str ---

    #[test]
    fn section_str_empty_rows_shows_none() {
        assert_eq!(section_str("My Section", &[]), "My Section:\n  (none)\n\n");
    }

    #[test]
    fn table_str_includes_platforms_status_and_source_columns() {
        let out = table_str(&[make_row("clipboard")]);
        assert!(out.contains("Platforms"), "Platforms header present");
        assert!(out.contains("Status"), "Status header present");
        assert!(out.contains("Source"), "Source header present");
        assert!(out.contains("clipboard"));
        assert!(out.contains("claude"));
        assert!(out.contains("guidelines"));
        assert!(out.contains("ok"));
    }

    #[test]
    fn table_str_uses_explicit_words_for_unversioned_and_missing_source() {
        let out = table_str(&[
            Row {
                name: "alpha".to_string(),
                installed_version: None,
                available_version: None,
                source: Some("guidelines".to_string()),
                platforms: vec!["claude".to_string()],
                status: ListStatus::Unversioned,
            },
            Row {
                name: "beta".to_string(),
                installed_version: Some("1.0.0".to_string()),
                available_version: None,
                source: None,
                platforms: vec![],
                status: ListStatus::SourceMissing,
            },
        ]);
        assert!(out.contains("unversioned"));
        assert!(out.contains("source missing"));
        assert!(!out.contains("✅"));
    }

    // --- list_status ---

    fn tracked_entry(source_checksum: &str, installed_checksum: &str) -> LockEntry {
        let mut entry = crate::test_support::make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0"),
            "guidelines",
            "agents/a.md",
            source_checksum,
        );
        entry.installed_checksum = installed_checksum.to_string();
        entry
    }

    fn version(v: &str) -> AvailableVersion {
        AvailableVersion::Version(v.to_string())
    }

    #[test]
    fn list_status_distinguishes_ok_outdated_unversioned_missing_and_deprecated() {
        let same = tracked_entry("sha256:same", "sha256:same");
        let old = tracked_entry("sha256:old", "sha256:old");
        assert_eq!(
            list_status(&version("1.0"), false, &[&same], Some("sha256:same")),
            ListStatus::Ok
        );
        assert_eq!(
            list_status(&version("2.0"), false, &[&old], Some("sha256:new")),
            ListStatus::Outdated
        );
        assert_eq!(
            list_status(&AvailableVersion::Unversioned, false, &[], None),
            ListStatus::Unversioned
        );
        assert_eq!(
            list_status(&AvailableVersion::SourceMissing, false, &[], None),
            ListStatus::SourceMissing
        );
        assert_eq!(
            list_status(&version("1.0"), true, &[&same], Some("sha256:same")),
            ListStatus::Deprecated
        );
    }

    #[test]
    fn list_status_outdated_when_checksum_differs_despite_matching_version_string() {
        // Regression: `list_status` used to compare version strings only, so a
        // source edited without a version bump incorrectly reported `Ok`.
        let old = tracked_entry("sha256:old", "sha256:old");
        assert_eq!(
            list_status(&version("1.0"), false, &[&old], Some("sha256:new")),
            ListStatus::Outdated,
            "matching version strings with a changed checksum must be Outdated, not Ok"
        );
    }

    #[test]
    fn list_status_untracked_is_outdated() {
        assert_eq!(
            list_status(&version("1.0"), false, &[], Some("sha256:x")),
            ListStatus::Outdated
        );
    }

    #[test]
    fn list_status_judges_each_tracked_entry_rather_than_an_aggregate() {
        let current = tracked_entry("sha256:now", "sha256:now");
        let stale_baseline = tracked_entry("sha256:stale", "sha256:now");
        let behind = tracked_entry("sha256:then", "sha256:then");
        assert_eq!(
            list_status(&version("1.0"), false, &[&current, &stale_baseline], Some("sha256:now")),
            ListStatus::Ok,
            "disagreeing baselines over current content are not outdated"
        );
        assert_eq!(
            list_status(&version("1.0"), false, &[&current, &behind], Some("sha256:now")),
            ListStatus::Outdated,
            "any tracked entry really behind makes the artifact outdated"
        );
    }

    // --- available_version ---

    #[test]
    fn available_version_prefers_install_source() {
        let infos = vec![
            SourceArtifactInfo {
                source_name: "a".to_string(),
                version: Some("1.0.0".to_string()),
                checksum: "x".to_string(),
                deprecated: false,
            },
            SourceArtifactInfo {
                source_name: "b".to_string(),
                version: Some("2.0.0".to_string()),
                checksum: "y".to_string(),
                deprecated: false,
            },
        ];
        assert_eq!(
            available_version(Some(&infos), Some("b")),
            AvailableVersion::Version("2.0.0".to_string())
        );
        assert_eq!(
            available_version(Some(&infos), Some("z")),
            AvailableVersion::Version("1.0.0".to_string())
        );
        assert_eq!(available_version(None, Some("a")), AvailableVersion::SourceMissing);
    }

    #[test]
    fn available_version_marks_unversioned_source_explicitly() {
        let infos = vec![SourceArtifactInfo {
            source_name: "a".to_string(),
            version: None,
            checksum: "x".to_string(),
            deprecated: false,
        }];
        assert_eq!(available_version(Some(&infos), Some("a")), AvailableVersion::Unversioned);
    }

    // --- end-to-end: cross-platform listing with platforms + clean source ---

    #[test]
    fn list_shows_platforms_and_clean_source_across_platforms() {
        let t = TestContext::new();
        setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file("/src/shared/SKILL.md", versioned_skill_content("s", "1.0.0"));
        for platform in [Platform::Codex, Platform::Pi] {
            let pv = t.paths.with_platform(platform);
            let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
            t.fs.add_file(
                dir.join("shared").join("SKILL.md"),
                versioned_skill_content("s", "1.0.0"),
            );
            let cs = crate::checksum::checksum_dir(&dir.join("shared"), &t.fs).unwrap();
            let entry = crate::test_support::make_lock_entry_with_checksum(
                ArtifactKind::Skill,
                Some("1.0.0"),
                "guidelines",
                "shared",
                &cs,
            );
            crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
                l.packages.insert("shared".to_string(), entry);
            })
            .unwrap();
        }

        let out = list_kind(ArtifactKind::Skill, false, &t.ctx()).unwrap();
        let rows = &out.rows[&InstallScope::Global];
        let row = rows.iter().find(|r| r.name == "shared").expect("listed");
        assert_eq!(
            row.source.as_deref(),
            Some("guidelines"),
            "source is the bare repo name, no path"
        );
        assert!(
            row.platforms.iter().any(|platform| platform == "codex")
                && row.platforms.iter().any(|platform| platform == "pi"),
            "platforms listed: {:?}",
            row.platforms
        );
        assert_eq!(row.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(row.available_version.as_deref(), Some("1.0.0"));
        assert_eq!(row.status, ListStatus::Ok);
    }

    #[test]
    fn list_reads_codex_agent_version_from_preserved_frontmatter() {
        use crate::test_support::metadata_versioned_agent_content;
        use std::path::Path;

        let t = TestContext::new();
        setup_source(&t.fs, &t.paths, "guidelines", "/src");
        let markdown = metadata_versioned_agent_content("reviewer", "Reviews code", "1.3.0");
        t.fs.add_file("/src/agents/reviewer.md", markdown.clone());
        let source_checksum = crate::checksum::checksum_artifact(
            Path::new("/src/agents/reviewer.md"),
            ArtifactKind::Agent,
            &t.fs,
        )
        .unwrap();

        // Claude gets the markdown verbatim; Codex gets the generated TOML,
        // whose only trace of the version is the preserved comment block.
        let codex_toml = cmx_core::agent::markdown_to_codex_toml(&markdown, "reviewer");
        for (platform, content) in [(Platform::Claude, markdown), (Platform::Codex, codex_toml)] {
            let pv = t.paths.with_platform(platform);
            let path = pv
                .require_installed_artifact_path(
                    ArtifactKind::Agent,
                    "reviewer",
                    InstallScope::Global,
                )
                .unwrap();
            t.fs.add_file(&path, content);
            let installed_checksum =
                crate::checksum::checksum_artifact(&path, ArtifactKind::Agent, &t.fs).unwrap();
            let mut entry = crate::test_support::make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.3.0"),
                "guidelines",
                "agents/reviewer.md",
                &source_checksum,
            );
            entry.installed_checksum = installed_checksum;
            crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
                l.packages.insert("reviewer".to_string(), entry);
            })
            .unwrap();
        }

        let out = list_kind(ArtifactKind::Agent, false, &t.ctx()).unwrap();
        let rows = &out.rows[&InstallScope::Global];
        let row = rows.iter().find(|r| r.name == "reviewer").expect("listed");
        assert!(
            row.platforms.iter().any(|p| p == "claude")
                && row.platforms.iter().any(|p| p == "codex"),
            "platforms listed: {:?}",
            row.platforms
        );
        assert_eq!(row.installed_version.as_deref(), Some("1.3.0"), "one agreed version");
        assert_eq!(row.available_version.as_deref(), Some("1.3.0"));
        assert_eq!(row.status, ListStatus::Ok);
    }

    #[test]
    fn list_reports_outdated_when_source_content_changes_without_version_bump() {
        let t = TestContext::new();
        setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file("/src/shared/SKILL.md", versioned_skill_content("s", "1.0.0"));

        let pv = t.paths.with_platform(Platform::Codex);
        let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
        t.fs.add_file(dir.join("shared").join("SKILL.md"), versioned_skill_content("s", "1.0.0"));
        let cs = crate::checksum::checksum_dir(&dir.join("shared"), &t.fs).unwrap();
        let entry = crate::test_support::make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            Some("1.0.0"),
            "guidelines",
            "shared",
            &cs,
        );
        crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
            l.packages.insert("shared".to_string(), entry);
        })
        .unwrap();

        // The source content changes but the version stays the same.
        t.fs.add_file("/src/shared/SKILL.md", versioned_skill_content("s updated", "1.0.0"));

        let out = list_kind(ArtifactKind::Skill, false, &t.ctx()).unwrap();
        let rows = &out.rows[&InstallScope::Global];
        let row = rows.iter().find(|r| r.name == "shared").expect("listed");
        assert_eq!(
            row.status,
            ListStatus::Outdated,
            "content changed without a version bump must be Outdated, matching `cmx outdated`"
        );
    }

    #[test]
    fn list_excludes_external_artifacts() {
        let t = TestContext::new();
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        let mine = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("mine");
        t.fs.add_file(mine.join("SKILL.md"), versioned_skill_content("m", "1.0.0"));
        let hermes = t.paths.with_platform(Platform::Hermes);
        let vendored = hermes
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("apple");
        t.fs.add_file(vendored.join("SKILL.md"), versioned_skill_content("a", "1.0.0"));
        let cfg = crate::types::CmxConfig {
            external: vec!["~/.hermes/skills".to_string()],
            ..Default::default()
        };
        crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let out = list_kind(ArtifactKind::Skill, false, &t.ctx()).unwrap();
        let names: Vec<&str> = out.rows.values().flatten().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"mine"), "your skill is listed");
        assert!(!names.contains(&"apple"), "external (Hermes) skill is excluded by default");

        let out_all = list_kind(ArtifactKind::Skill, true, &t.ctx()).unwrap();
        let names_all: Vec<&str> =
            out_all.rows.values().flatten().map(|r| r.name.as_str()).collect();
        assert!(names_all.contains(&"apple"), "list --all includes external");
    }

    // --- per-platform baselines that disagree ---

    /// Put byte-identical copies of `/src/voice` in each platform's global
    /// skill dir, tracked with the given `(source_checksum, installed_checksum)`.
    fn install_voice_copies(t: &TestContext, baselines: &[(Platform, &str, &str)]) {
        for &(platform, source_checksum, installed_checksum) in baselines {
            let pv = t.paths.with_platform(platform);
            let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
            t.fs.add_file(
                dir.join("voice").join("SKILL.md"),
                versioned_skill_content("v", "1.3.0"),
            );
            let mut entry = crate::test_support::make_lock_entry_with_checksum(
                ArtifactKind::Skill,
                Some("1.3.0"),
                "guidelines",
                "voice",
                source_checksum,
            );
            entry.installed_checksum = installed_checksum.to_string();
            crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
                l.packages.insert("voice".to_string(), entry);
            })
            .unwrap();
        }
    }

    fn voice_source(t: &TestContext) -> String {
        setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file("/src/voice/SKILL.md", versioned_skill_content("v", "1.3.0"));
        crate::checksum::checksum_dir(std::path::Path::new("/src/voice"), &t.fs).unwrap()
    }

    fn voice_status(t: &TestContext) -> ListStatus {
        let out = list_kind(ArtifactKind::Skill, false, &t.ctx()).unwrap();
        out.rows[&InstallScope::Global]
            .iter()
            .find(|r| r.name == "voice")
            .expect("listed")
            .status
    }

    #[test]
    fn list_and_outdated_agree_a_stale_source_baseline_over_current_content_is_ok() {
        // Regression from real data: claude's baseline is current; codex's and
        // hermes' recorded source_checksum is stale, but the content they
        // installed is the source's current content.
        let t = TestContext::new();
        let current = voice_source(&t);
        install_voice_copies(
            &t,
            &[
                (Platform::Claude, &current, &current),
                (Platform::Codex, "sha256:e5ef-stale", &current),
                (Platform::Hermes, "sha256:e5ef-stale", &current),
            ],
        );

        assert_eq!(voice_status(&t), ListStatus::Ok);
        for platform in [Platform::Claude, Platform::Codex, Platform::Hermes] {
            let pv = t.paths.with_platform(platform);
            let report = crate::outdated::outdated(&t.ctx().with_paths(&pv)).unwrap();
            assert!(
                report.0.iter().all(|r| r.name != "voice"),
                "{platform}: outdated must omit it: {:?}",
                report.0
            );
        }
    }

    #[test]
    fn list_reports_outdated_when_one_disagreeing_copy_is_really_behind() {
        let t = TestContext::new();
        let current = voice_source(&t);
        install_voice_copies(
            &t,
            &[
                (Platform::Claude, &current, &current),
                (Platform::Codex, "sha256:old-source", "sha256:old-install"),
            ],
        );

        assert_eq!(voice_status(&t), ListStatus::Outdated);
    }
}
