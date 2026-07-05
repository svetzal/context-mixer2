use std::path::Path;

use serde_json::{Value, json};

use crate::cmx_config::ConfigShowResult;
use crate::info::ArtifactInfo;
use crate::list::{ListKindOutput, ListOutput, Row};
use crate::outdated::OutdatedReport;
use crate::search::SearchOutput;
use crate::sets::{SetListResult, SetShowResult};
use crate::source::{SourceBrowseResult, SourceListResult};
use crate::types::{ArtifactKind, InstallScope};

fn list_row_json(kind: ArtifactKind, scope: InstallScope, row: &Row) -> Value {
    json!({
        "kind": kind.to_string(),
        "scope": scope.label(),
        "name": row.name,
        "installed_version": row.installed,
        "available_version": row.available,
        "source": row.source,
        "tools": row.tools,
        "status": row.status,
    })
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

pub fn list_json(output: &ListOutput) -> Value {
    let mut artifacts = flatten_rows(&output.agents, ArtifactKind::Agent);
    artifacts.extend(flatten_rows(&output.skills, ArtifactKind::Skill));
    json!({ "artifacts": artifacts })
}

pub fn list_kind_json(output: &ListKindOutput) -> Value {
    json!({
        "kind": output.kind.to_string(),
        "artifacts": flatten_rows(&output.rows, output.kind),
    })
}

pub fn outdated_json(report: &OutdatedReport) -> Value {
    json!({
        "artifacts": report.0.iter().map(|row| json!({
            "kind": row.kind.to_string(),
            "scope": row.scope.label(),
            "name": row.name,
            "installed_version": row.installed_version,
            "available_version": row.available_version,
            "source": row.source,
            "status": row.status,
        })).collect::<Vec<_>>(),
    })
}

pub fn search_json(output: &SearchOutput) -> Value {
    serde_json::to_value(output).expect("SearchOutput is serializable")
}

pub fn info_json(info: &ArtifactInfo) -> Value {
    json!({
        "name": info.name,
        "kind": info.kind.to_string(),
        "scope": info.scope,
        "path": info.path.display().to_string(),
        "version": info.version,
        "installed_at": info.installed_at,
        "source": info.source_display,
        "source_checksum": info.source_checksum,
        "installed_checksum": info.installed_checksum,
        "disk_checksum": info.disk_checksum,
        "locally_modified": info.locally_modified,
        "untracked": info.untracked,
        "deprecation": info.deprecation,
        "available_version": info.available_version,
        "files": info.skill_files,
        "activation_description": info.activates_when,
        "summary": info.summary,
    })
}

pub fn source_list_json(result: &SourceListResult) -> Value {
    json!({
        "sources": result.entries.iter().map(|entry| json!({
            "name": entry.name,
            "type": entry.kind,
            "location": entry.location,
        })).collect::<Vec<_>>(),
    })
}

pub fn source_browse_json(result: &SourceBrowseResult) -> Value {
    json!({
        "source": result.source_name,
        "agents": result.agents.iter().map(|artifact| json!({
            "name": artifact.name,
            "version": artifact.version,
            "description": artifact.description,
            "deprecation": artifact.deprecation,
        })).collect::<Vec<_>>(),
        "skills": result.skills.iter().map(|skill| json!({
            "name": skill.name,
            "version": skill.version,
            "description": skill.description,
            "deprecation": skill.deprecation,
            "files": skill.files,
        })).collect::<Vec<_>>(),
    })
}

pub fn set_list_json(result: &SetListResult, scope: InstallScope) -> Value {
    json!({
        "scope": scope.label(),
        "sets": result.entries,
    })
}

pub fn set_show_json(result: &SetShowResult, scope: InstallScope) -> Value {
    json!({
        "scope": scope.label(),
        "name": result.name,
        "description": result.description,
        "state": result.state,
        "footprint_chars": result.footprint_chars,
        "members": result.members,
    })
}

pub fn config_show_json(result: &ConfigShowResult) -> Value {
    let mut value = serde_json::to_value(result).expect("ConfigShowResult is serializable");
    let map = value.as_object_mut().expect("ConfigShowResult serializes to an object");
    map.insert("platforms_inferred".to_string(), json!(result.platforms.is_empty()));
    value
}

pub fn home_path_json(path: &Path) -> Value {
    json!({ "path": path.display().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::SkillFileEntry;
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
