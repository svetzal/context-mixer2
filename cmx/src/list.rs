use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::context::AppContext;
use crate::doctor::{self, ArtifactState};
use crate::source_iter::{self, SourceArtifactInfo};
use crate::table::Table;
use crate::types::{ArtifactKind, InstallScope};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListStatus {
    Ok,
    Outdated,
    Unversioned,
    SourceMissing,
    Deprecated,
}

impl ListStatus {
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
    pub name: String,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    /// The source it came from (repo name only, no path).
    pub source: Option<String>,
    /// The platforms cmx tracks it for.
    pub platforms: Vec<String>,
    pub status: ListStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListKindOutput {
    pub kind: ArtifactKind,
    pub rows: BTreeMap<InstallScope, Vec<Row>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListOutput {
    pub agents: BTreeMap<InstallScope, Vec<Row>>,
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
                    display_installed_version(r.installed_version.as_deref()),
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
        out.push_str("  (none)\n");
    } else {
        out.push_str(&table_str(rows));
    }
    out.push('\n');
    out
}

fn display_installed_version(version: Option<&str>) -> String {
    version.unwrap_or("unversioned").to_string()
}

fn display_available_version(version: Option<&str>, status: ListStatus) -> String {
    match version {
        Some(version) => version.to_string(),
        None if status == ListStatus::SourceMissing => "source missing".to_string(),
        None => "unversioned".to_string(),
    }
}

fn display_source(source: Option<&str>) -> String {
    source.unwrap_or("no source").to_string()
}

fn list_status(
    installed: Option<&str>,
    available: &AvailableVersion,
    deprecated: bool,
) -> ListStatus {
    if deprecated {
        return ListStatus::Deprecated;
    }

    match (installed, available) {
        (_, AvailableVersion::SourceMissing) => ListStatus::SourceMissing,
        (_, AvailableVersion::Unversioned) => ListStatus::Unversioned,
        (Some(installed), AvailableVersion::Version(available)) if installed == available => {
            ListStatus::Ok
        }
        _ => ListStatus::Outdated,
    }
}

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
    let report = doctor::survey(true, ctx)?;
    let source_versions = source_iter::all_with_checksums(ctx)?;

    let mut by_scope: BTreeMap<InstallScope, Vec<Row>> = BTreeMap::new();
    for a in report
        .artifacts
        .iter()
        .filter(|a| a.kind == kind && (include_external || a.state != ArtifactState::External))
    {
        let infos = source_versions.get(&a.name);
        let available = available_version(infos, a.source.as_deref());
        let deprecated =
            preferred_source_info(infos, a.source.as_deref()).is_some_and(|i| i.deprecated);

        by_scope.entry(a.scope).or_default().push(Row {
            name: a.name.clone(),
            installed_version: a.version.clone(),
            available_version: match &available {
                AvailableVersion::Version(version) => Some(version.clone()),
                AvailableVersion::Unversioned | AvailableVersion::SourceMissing => None,
            },
            source: a.source.clone(),
            platforms: a.tools.iter().map(ToString::to_string).collect(),
            status: list_status(a.version.as_deref(), &available, deprecated),
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

    #[test]
    fn list_status_distinguishes_ok_outdated_unversioned_missing_and_deprecated() {
        assert_eq!(
            list_status(Some("1.0"), &AvailableVersion::Version("1.0".to_string()), false),
            ListStatus::Ok
        );
        assert_eq!(
            list_status(Some("1.0"), &AvailableVersion::Version("2.0".to_string()), false),
            ListStatus::Outdated
        );
        assert_eq!(
            list_status(Some("1.0"), &AvailableVersion::Unversioned, false),
            ListStatus::Unversioned
        );
        assert_eq!(
            list_status(Some("1.0"), &AvailableVersion::SourceMissing, false),
            ListStatus::SourceMissing
        );
        assert_eq!(
            list_status(Some("1.0"), &AvailableVersion::Version("1.0".to_string()), true),
            ListStatus::Deprecated
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
        use crate::platform::Platform;

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
    fn list_excludes_external_artifacts() {
        use crate::platform::Platform;

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
}
