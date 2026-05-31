use anyhow::Result;
use std::collections::BTreeMap;

use crate::context::AppContext;
use crate::doctor::{self, ArtifactState};
use crate::source_iter::{self, SourceArtifactInfo};
use crate::table::Table;
use crate::types::{ArtifactKind, InstallScope, display_version};

/// One row in the listing — a logical artifact (grouped across the tools it's
/// installed for, via [`crate::doctor`]).
pub struct Row {
    pub name: String,
    pub installed: String,
    pub available: String,
    /// The source it came from (repo name only, no path).
    pub source: String,
    /// The tools cmx tracks it for, joined (e.g. `claude, codex`), or `-`.
    pub tools: String,
    pub status: &'static str,
}

pub struct ListKindOutput {
    pub kind: ArtifactKind,
    pub rows: BTreeMap<InstallScope, Vec<Row>>,
}

pub struct ListOutput {
    pub agents: BTreeMap<InstallScope, Vec<Row>>,
    pub skills: BTreeMap<InstallScope, Vec<Row>>,
}

pub(crate) fn table_str(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    Table {
        headers: vec!["Name", "Installed", "Available", "Source", "Tools"],
        padded_cols: 5,
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.installed.clone(),
                    r.available.clone(),
                    r.source.clone(),
                    r.tools.clone(),
                    r.status.to_string(),
                ]
            })
            .collect(),
    }
    .render()
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

fn status_indicator(
    installed: Option<&str>,
    available: Option<&str>,
    deprecated: bool,
) -> &'static str {
    if deprecated {
        return "⛔";
    }
    match (installed, available) {
        (None | Some(_), None) => " ",
        (Some(i), Some(a)) if i == a => "✅",
        _ => "⚠️",
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
/// per logical artifact, with the tools it's tracked for and an available-version
/// comparison drawn from the registered sources.
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
        let deprecated = infos.is_some_and(|v| v.iter().any(|i| i.deprecated));

        let installed = a.version.as_deref();
        let tools = if a.tools.is_empty() {
            "-".to_string()
        } else {
            a.tools.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        };
        by_scope.entry(a.scope).or_default().push(Row {
            name: a.name.clone(),
            installed: display_version(installed).to_string(),
            available: display_version(available.as_deref()).to_string(),
            source: a.source.clone().unwrap_or_else(|| "-".to_string()),
            tools,
            status: status_indicator(installed, available.as_deref(), deprecated),
        });
    }
    Ok(by_scope)
}

/// The version a source offers for an artifact: prefer the source it was
/// installed from (`from`), else the first source that provides it.
fn available_version(
    infos: Option<&Vec<SourceArtifactInfo>>,
    from: Option<&str>,
) -> Option<String> {
    let infos = infos?;
    infos
        .iter()
        .find(|i| from.is_some_and(|f| i.source_name == f))
        .or_else(|| infos.first())
        .and_then(|i| i.version.clone())
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
            installed: "1.0.0".to_string(),
            available: "1.0.0".to_string(),
            source: "guidelines".to_string(),
            tools: "claude".to_string(),
            status: "✅",
        }
    }

    // --- section_str / table_str ---

    #[test]
    fn section_str_empty_rows_shows_none() {
        assert_eq!(section_str("My Section", &[]), "My Section:\n  (none)\n\n");
    }

    #[test]
    fn table_str_includes_tools_and_source_columns() {
        let out = table_str(&[make_row("clipboard")]);
        assert!(out.contains("Tools"), "Tools header present");
        assert!(out.contains("Source"), "Source header present");
        assert!(out.contains("clipboard"));
        assert!(out.contains("claude"));
        assert!(out.contains("guidelines"));
    }

    // --- status_indicator ---

    #[test]
    fn status_indicator_up_to_date_behind_and_deprecated() {
        assert_eq!(status_indicator(Some("1.0"), Some("1.0"), false), "✅");
        assert_eq!(status_indicator(Some("1.0"), Some("2.0"), false), "⚠️");
        assert_eq!(status_indicator(Some("1.0"), None, false), " ");
        assert_eq!(status_indicator(Some("1.0"), Some("1.0"), true), "⛔");
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
        assert_eq!(available_version(Some(&infos), Some("b")), Some("2.0.0".to_string()));
        // Unknown source falls back to the first.
        assert_eq!(available_version(Some(&infos), Some("z")), Some("1.0.0".to_string()));
        assert_eq!(available_version(None, Some("a")), None);
    }

    // --- end-to-end: cross-platform listing with tools + clean source ---

    #[test]
    fn list_shows_tools_and_clean_source_across_platforms() {
        use crate::platform::Platform;

        let t = TestContext::new();
        // A source provides "shared"; it's tracked for two cohort tools (codex, pi)
        // in the shared .agents/skills dir.
        setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file("/src/shared/SKILL.md", versioned_skill_content("s", "1.0.0"));
        for platform in [Platform::Codex, Platform::Pi] {
            let pv = t.paths.with_platform(platform);
            let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global);
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
        assert_eq!(row.source, "guidelines", "source is the bare repo name, no path");
        assert!(
            row.tools.contains("codex") && row.tools.contains("pi"),
            "tools listed: {}",
            row.tools
        );
        assert_eq!(row.installed, "1.0.0");
        assert_eq!(row.available, "1.0.0");
    }

    #[test]
    fn list_excludes_external_artifacts() {
        use crate::platform::Platform;

        let t = TestContext::new();
        crate::test_support::setup_empty_sources(&t.fs, &t.paths);
        // A hand-authored skill (mine) and a vendor skill in ~/.hermes/skills.
        let mine = t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global).join("mine");
        t.fs.add_file(mine.join("SKILL.md"), versioned_skill_content("m", "1.0.0"));
        let hermes = t.paths.with_platform(Platform::Hermes);
        let vendored = hermes.install_dir(ArtifactKind::Skill, InstallScope::Global).join("apple");
        t.fs.add_file(vendored.join("SKILL.md"), versioned_skill_content("a", "1.0.0"));
        // Declare the Hermes directory external.
        let cfg = crate::types::CmxConfig {
            external: vec!["~/.hermes/skills".to_string()],
            ..Default::default()
        };
        crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let out = list_kind(ArtifactKind::Skill, false, &t.ctx()).unwrap();
        let names: Vec<&str> = out.rows.values().flatten().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"mine"), "your skill is listed");
        assert!(!names.contains(&"apple"), "external (Hermes) skill is excluded by default");

        // --all includes external.
        let out_all = list_kind(ArtifactKind::Skill, true, &t.ctx()).unwrap();
        let names_all: Vec<&str> =
            out_all.rows.values().flatten().map(|r| r.name.as_str()).collect();
        assert!(names_all.contains(&"apple"), "list --all includes external");
    }
}
