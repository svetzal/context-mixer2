//! Shared JSON output formatting helpers, a submodule of
//! `cmx/src/display/mod.rs`.

use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cmx_config::ConfigShowResult;
use crate::info::ArtifactInfo;
use crate::list::{ListKindOutput, ListOutput, Row};
use crate::outdated::{OutdatedReport, OutdatedRow};
use crate::search::SearchOutput;
use crate::sets::{SetListEntry, SetListResult, SetShowResult};
use crate::source::{SourceBrowseResult, SourceListResult};
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// Projection of one list row plus its kind and scope — the single home for
/// the `cmx list --json` per-artifact contract. `Row` already derives
/// `Serialize`; this wrapper adds the two fields that live outside `Row`.
#[derive(Serialize)]
struct ListRowJson<'a> {
    kind: ArtifactKind,
    scope: InstallScope,
    #[serde(flatten)]
    row: &'a Row,
}

fn list_row_json(kind: ArtifactKind, scope: InstallScope, row: &Row) -> Value {
    serde_json::to_value(ListRowJson { kind, scope, row }).expect("ListRowJson is serializable")
}

fn flatten_rows(
    rows: &std::collections::BTreeMap<InstallScope, Vec<Row>>,
    kind: ArtifactKind,
) -> Vec<Value> {
    rows.iter()
        .flat_map(|(scope, scoped_rows)| {
            scoped_rows
                .iter()
                .map(|row| list_row_json(kind, *scope, row))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Serialize `cmx list`'s combined agent/skill output to the `--json` contract.
pub fn list_json(output: &ListOutput) -> Value {
    let mut artifacts = flatten_rows(&output.agents, ArtifactKind::Agent);
    artifacts.extend(flatten_rows(&output.skills, ArtifactKind::Skill));
    json!({ "artifacts": artifacts })
}

/// Serialize `cmx agent list` / `cmx skill list`'s single-kind output to the
/// `--json` contract.
pub fn list_kind_json(output: &ListKindOutput) -> Value {
    json!({
        "kind": output.kind.to_string(),
        "artifacts": flatten_rows(&output.rows, output.kind),
    })
}

// ---------------------------------------------------------------------------
// Outdated
// ---------------------------------------------------------------------------

/// Envelope for `OutdatedRow` items so `outdated_json` has a single home for
/// the `"artifacts"` wrapper key.
#[derive(Serialize)]
struct OutdatedJson<'a> {
    artifacts: &'a [OutdatedRow],
}

/// Serialize `cmx outdated`'s report to the `--json` contract.
pub fn outdated_json(report: &OutdatedReport) -> Value {
    serde_json::to_value(OutdatedJson {
        artifacts: &report.0,
    })
    .expect("OutdatedJson is serializable")
}

// ---------------------------------------------------------------------------
// Search / Info
// ---------------------------------------------------------------------------

/// Serialize `cmx search`'s output to the `--json` contract.
pub fn search_json(output: &SearchOutput) -> Value {
    serde_json::to_value(output).expect("SearchOutput is serializable")
}

/// Serialize `ArtifactInfo` to the `cmx info --json` contract.
///
/// Field renames (`source_display` → `"source"`, `skill_files` → `"files"`,
/// `activates_when` → `"activation_description"`) and the deliberate omission
/// of `summary_error` are encoded as serde attributes on `ArtifactInfo`
/// itself — this is the single source of truth for those decisions.
pub fn info_json(info: &ArtifactInfo) -> Value {
    serde_json::to_value(info).expect("ArtifactInfo is serializable")
}

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

/// Serialize `SourceListResult` to the `cmx source list --json` contract.
///
/// The `"sources"` wrapper key and the `"type"` field rename are encoded as
/// serde attributes on `SourceListResult` / `SourceListEntry`.
pub fn source_list_json(result: &SourceListResult) -> Value {
    serde_json::to_value(result).expect("SourceListResult is serializable")
}

/// Serialize `SourceBrowseResult` to the `cmx source browse --json` contract.
///
/// The `"source"` rename and the omission of `deprecation_display` are encoded
/// as serde attributes on `SourceBrowseResult` / `BrowseArtifact` / `BrowseSkill`.
pub fn source_browse_json(result: &SourceBrowseResult) -> Value {
    serde_json::to_value(result).expect("SourceBrowseResult is serializable")
}

// ---------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------

/// Projection that adds the `scope` field (not part of `SetListResult`) to the
/// `cmx set list --json` output. The `"sets"` key rename is declared here.
#[derive(Serialize)]
struct SetListJson<'a> {
    scope: InstallScope,
    #[serde(rename = "sets")]
    entries: &'a [SetListEntry],
}

/// Serialize `cmx set list`'s output to the `--json` contract.
pub fn set_list_json(result: &SetListResult, scope: InstallScope) -> Value {
    serde_json::to_value(SetListJson {
        scope,
        entries: &result.entries,
    })
    .expect("SetListJson is serializable")
}

/// Projection that adds the `scope` field (not part of `SetShowResult`) to the
/// `cmx set show --json` output.
#[derive(Serialize)]
struct SetShowJson<'a> {
    scope: InstallScope,
    #[serde(flatten)]
    result: &'a SetShowResult,
}

/// Serialize `cmx set show`'s output to the `--json` contract.
pub fn set_show_json(result: &SetShowResult, scope: InstallScope) -> Value {
    serde_json::to_value(SetShowJson { scope, result }).expect("SetShowJson is serializable")
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Projection that adds `platforms_inferred` (a derived field not stored in
/// `ConfigShowResult`) to the `cmx config show --json` output. Having it here
/// removes the mutable-json-object mutation from `config_show_json`.
#[derive(Serialize)]
struct ConfigShowJson<'a> {
    #[serde(flatten)]
    result: &'a ConfigShowResult,
    platforms_inferred: bool,
}

/// Serialize `cmx config show`'s output to the `--json` contract.
pub fn config_show_json(result: &ConfigShowResult) -> Value {
    serde_json::to_value(ConfigShowJson {
        platforms_inferred: result.platforms.is_empty(),
        result,
    })
    .expect("ConfigShowJson is serializable")
}

// ---------------------------------------------------------------------------
// Home path
// ---------------------------------------------------------------------------

/// Serialize `cmx home`'s resolved path to the `--json` contract.
pub fn home_path_json(path: &Path) -> Value {
    json!({ "path": path.display().to_string() })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::SkillFileEntry;
    use crate::list::ListStatus;
    use crate::outdated::{OutdatedReport, OutdatedRow, OutdatedStatus};
    use crate::types::{Deprecation, SetState};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn list_json_empty_uses_empty_artifacts_array() {
        let value = list_json(&ListOutput {
            agents: BTreeMap::new(),
            skills: BTreeMap::new(),
        });
        assert_eq!(value["artifacts"], json!([]));
    }

    #[test]
    fn list_json_uses_null_versions_and_semantic_status() {
        let value = list_json(&ListOutput {
            agents: BTreeMap::new(),
            skills: BTreeMap::from([(
                InstallScope::Global,
                vec![Row {
                    name: "focus-skill".to_string(),
                    installed_version: None,
                    available_version: None,
                    source: Some("guidelines".to_string()),
                    platforms: vec!["claude".to_string()],
                    status: ListStatus::Unversioned,
                }],
            )]),
        });
        assert!(value["artifacts"][0]["installed_version"].is_null());
        assert!(value["artifacts"][0]["available_version"].is_null());
        assert_eq!(value["artifacts"][0]["platforms"], json!(["claude"]));
        assert_eq!(value["artifacts"][0]["status"], "unversioned");
    }

    #[test]
    fn list_row_json_key_set_is_stable() {
        let row = Row {
            name: "n".to_string(),
            installed_version: Some("1.0.0".to_string()),
            available_version: Some("1.0.0".to_string()),
            source: Some("src".to_string()),
            platforms: vec!["claude".to_string()],
            status: ListStatus::Ok,
        };
        let value = list_row_json(ArtifactKind::Skill, InstallScope::Global, &row);
        let obj = value.as_object().unwrap();
        for key in [
            "kind",
            "scope",
            "name",
            "installed_version",
            "available_version",
            "source",
            "platforms",
            "status",
        ] {
            assert!(obj.contains_key(key), "list row missing key: {key}");
        }
        assert_eq!(obj.len(), 8, "unexpected extra keys in list row: {value}");
    }

    #[test]
    fn outdated_json_uses_null_versions_and_locally_modified_flag() {
        let value = outdated_json(&OutdatedReport(vec![OutdatedRow {
            name: "focus-skill".to_string(),
            kind: ArtifactKind::Skill,
            scope: InstallScope::Global,
            installed_version: None,
            available_version: None,
            source: "guidelines".to_string(),
            status: OutdatedStatus::Changed,
            locally_modified: true,
        }]));
        assert!(value["artifacts"][0]["installed_version"].is_null());
        assert!(value["artifacts"][0]["available_version"].is_null());
        assert_eq!(value["artifacts"][0]["status"], "changed");
        assert_eq!(value["artifacts"][0]["locally_modified"], json!(true));
    }

    #[test]
    fn outdated_row_key_set_is_stable() {
        let row = OutdatedRow {
            name: "n".to_string(),
            kind: ArtifactKind::Skill,
            scope: InstallScope::Global,
            installed_version: None,
            available_version: None,
            source: "src".to_string(),
            status: OutdatedStatus::Changed,
            locally_modified: false,
        };
        let value = outdated_json(&OutdatedReport(vec![row]));
        let obj = value["artifacts"][0].as_object().unwrap();
        for key in [
            "kind",
            "scope",
            "name",
            "installed_version",
            "available_version",
            "source",
            "status",
            "locally_modified",
        ] {
            assert!(obj.contains_key(key), "outdated row missing key: {key}");
        }
        assert_eq!(obj.len(), 8, "unexpected extra keys in outdated row: {value}");
    }

    #[test]
    fn info_json_uses_null_summary_when_missing() {
        let value = info_json(&ArtifactInfo {
            name: "my-skill".to_string(),
            kind: ArtifactKind::Skill,
            scope: "global",
            path: PathBuf::from("/tmp/my-skill"),
            version: Some("1.0.0".to_string()),
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: false,
            deprecation: Some(Deprecation {
                reason: Some("old".to_string()),
                replacement: Some("new-skill".to_string()),
            }),
            available_version: None,
            skill_files: vec![SkillFileEntry {
                name: "SKILL.md".to_string(),
                is_dir: false,
                indent_level: 0,
            }],
            activates_when: Some("Use this skill when you need focus".to_string()),
            summary: None,
            summary_error: Some("provider exploded".to_string()),
        });
        assert!(value["summary"].is_null());
        assert_eq!(value["activation_description"], "Use this skill when you need focus");
        assert!(value.get("summary_error").is_none());
    }

    #[test]
    fn info_json_key_set_is_stable() {
        let value = info_json(&ArtifactInfo {
            name: "n".to_string(),
            kind: ArtifactKind::Skill,
            scope: "global",
            path: PathBuf::from("/tmp/n"),
            version: None,
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: false,
            deprecation: None,
            available_version: None,
            skill_files: vec![],
            activates_when: None,
            summary: None,
            summary_error: None,
        });
        let obj = value.as_object().unwrap();
        for key in [
            "name",
            "kind",
            "scope",
            "path",
            "version",
            "installed_at",
            "source",
            "source_checksum",
            "installed_checksum",
            "disk_checksum",
            "locally_modified",
            "untracked",
            "deprecation",
            "available_version",
            "files",
            "activation_description",
            "summary",
        ] {
            assert!(obj.contains_key(key), "info json missing key: {key}");
        }
        assert!(
            obj.get("summary_error").is_none(),
            "summary_error must be absent from json output"
        );
        assert_eq!(obj.len(), 17, "unexpected extra keys in info json: {value}");
    }

    #[test]
    fn set_list_json_empty_uses_empty_sets_array() {
        let value = set_list_json(&SetListResult { entries: vec![] }, InstallScope::Global);
        assert_eq!(value["sets"], json!([]));
        assert_eq!(value["scope"], "global");
    }

    #[test]
    fn config_show_json_marks_inferred_platforms() {
        let value = config_show_json(&ConfigShowResult {
            gateway: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            external: vec![],
            platforms: vec![],
        });
        assert_eq!(value["platforms_inferred"], json!(true));
    }

    #[test]
    fn set_show_json_preserves_members() {
        let value = set_show_json(
            &SetShowResult {
                name: "daily".to_string(),
                description: Some("Daily tools".to_string()),
                state: SetState::Active,
                members: vec![],
                footprint_chars: 42,
            },
            InstallScope::Global,
        );
        assert_eq!(value["name"], "daily");
        assert_eq!(value["members"], json!([]));
    }
}
