use serde::Serialize;
use serde_json::Value;

use crate::doctor::DoctorReport;

// ---------------------------------------------------------------------------
// JSON projection types — the single home for the `cmx doctor --json` contract
// ---------------------------------------------------------------------------

/// Per-location row in the `"locations"` array.
#[derive(Serialize)]
struct DoctorLocationJson {
    path: String,
    platform: Option<String>,
    version: Option<String>,
    state: &'static str,
}

/// Per-artifact object in the `"artifacts"` array.
///
/// The `version` / `versions` XOR is expressed by the constructor
/// (`from_artifact`): when the artifact has a single agreed-upon version,
/// `version` is set and `versions` is left empty (skipped); otherwise
/// `version` is left `None` (skipped) and `versions` carries all distinct
/// versions. This encodes the XOR rule in exactly one place.
#[derive(Serialize)]
struct DoctorArtifactJson {
    kind: crate::types::ArtifactKind,
    name: String,
    scope: crate::types::InstallScope,
    state: &'static str,
    source: Option<String>,
    tools: Vec<String>,
    diverged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    versions: Vec<String>,
    locations: Vec<DoctorLocationJson>,
}

impl DoctorArtifactJson {
    fn from_artifact(a: &crate::doctor::DoctorArtifact, rows: &[crate::doctor::DoctorRow]) -> Self {
        let locations = crate::doctor::location_members(a, rows)
            .into_iter()
            .map(|m| DoctorLocationJson {
                path: m.location.display().to_string(),
                platform: m.platform.map(|p| p.to_string()),
                version: m.version,
                state: m.state_label,
            })
            .collect();
        Self {
            kind: a.kind,
            name: a.name.clone(),
            scope: a.scope,
            state: a.state.label(),
            source: a.source.clone(),
            tools: a.tools.iter().map(ToString::to_string).collect(),
            diverged: a.diverged,
            // XOR: emit "version" when all copies agree, "versions" when they differ.
            version: a.version.clone(),
            versions: if a.version.is_some() {
                vec![]
            } else {
                a.versions.clone()
            },
            locations,
        }
    }
}

/// Top-level `cmx doctor --json` output.
#[derive(Serialize)]
struct DoctorJson<'a> {
    scope: &'static str,
    platforms_surveyed: usize,
    showing: &'static str,
    summary: crate::doctor::StateCounts,
    artifacts: Vec<DoctorArtifactJson>,
    set_inconsistencies: &'a [crate::doctor::SetInconsistency],
}

/// Project the survey to the machine-readable schema documented for
/// `cmx doctor --json`. Mirrors the human `Display` impl's content and
/// selection (via [`super::shown_artifacts`]), but structures divergence as a
/// `locations` array instead of free-text prose.
///
/// Every field-name and value-encoding decision is expressed as serde
/// attributes on the projection types above — there is exactly one home
/// for the `--json` contract.
pub fn doctor_json(report: &DoctorReport) -> Value {
    let shown = super::shown_artifacts(report);
    let artifacts = shown
        .iter()
        .map(|a| DoctorArtifactJson::from_artifact(a, &report.rows))
        .collect();

    serde_json::to_value(DoctorJson {
        scope: if report.included_local {
            "global+local"
        } else {
            "global"
        },
        platforms_surveyed: report.surveyed_platforms,
        showing: if report.show_all {
            "all"
        } else {
            "needs_attention"
        },
        summary: report.counts(),
        artifacts,
        set_inconsistencies: &report.set_inconsistencies,
    })
    .expect("DoctorJson is serializable")
}

// ---------------------------------------------------------------------------
// JSON-specific tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::doctor_json;
    use crate::doctor::{ArtifactState, DoctorArtifact, DoctorReport, DoctorRow};
    use crate::platform::Platform;
    use crate::types::{ArtifactKind, InstallScope};
    use std::path::PathBuf;

    fn orphan_artifact(name: &str) -> DoctorArtifact {
        DoctorArtifact {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            state: ArtifactState::Orphaned,
            version: Some("1.0.0".to_string()),
            versions: vec!["1.0.0".to_string()],
            tools: vec![Platform::Claude],
            source: None,
            locations: vec![PathBuf::from("/home/u/.claude/skills")],
            diverged: false,
        }
    }

    fn skill_row(
        name: &str,
        location: &str,
        platforms: Vec<Platform>,
        state: ArtifactState,
        version: Option<&str>,
    ) -> DoctorRow {
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            location: PathBuf::from(location),
            platforms,
            tracked_for: vec![],
            state,
            version: version.map(str::to_string),
            source: None,
            content_checksum: format!("sha256:{name}:{location}:{}", version.unwrap_or("none")),
        }
    }

    fn diverged_report_with_locations() -> DoctorReport {
        let mut art = orphan_artifact("hopper-coordinator");
        art.state = ArtifactState::External;
        art.diverged = true;
        art.version = None;
        art.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        art.locations = vec![
            PathBuf::from("/u/.claude/skills"),
            PathBuf::from("/u/.agents/skills"),
        ];
        DoctorReport {
            rows: vec![
                skill_row(
                    "hopper-coordinator",
                    "/u/.claude/skills",
                    vec![Platform::Claude],
                    ArtifactState::External,
                    Some("3.3.0"),
                ),
                skill_row(
                    "hopper-coordinator",
                    "/u/.agents/skills",
                    vec![Platform::Codex],
                    ArtifactState::External,
                    Some("3.2.0"),
                ),
            ],
            artifacts: vec![art],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
            set_inconsistencies: vec![],
        }
    }

    #[test]
    fn doctor_json_schema_pins_shape() {
        let r = diverged_report_with_locations();
        let value = doctor_json(&r);

        assert_eq!(value["scope"], "global");
        assert_eq!(value["platforms_surveyed"], 13);
        assert_eq!(value["showing"], "needs_attention");

        let summary = &value["summary"];
        for key in [
            "tracked",
            "drifted",
            "untracked",
            "orphaned",
            "external",
            "missing",
            "diverged",
            "set_inconsistent",
        ] {
            assert!(summary.get(key).is_some(), "summary missing {key}: {value}");
        }
        assert_eq!(summary["diverged"], 1);
        assert_eq!(summary["set_inconsistent"], 0);
        assert!(value["set_inconsistencies"].as_array().expect("array").is_empty());

        let artifacts = value["artifacts"].as_array().expect("artifacts array");
        assert_eq!(artifacts.len(), 1);
        let a = &artifacts[0];
        assert_eq!(a["name"], "hopper-coordinator");
        assert_eq!(a["diverged"], true);
        assert_eq!(a["versions"], serde_json::json!(["3.2.0", "3.3.0"]));

        let locations = a["locations"].as_array().expect("locations array");
        assert_eq!(locations.len(), 2);
        for loc in locations {
            assert!(loc.get("path").is_some());
            assert!(loc.get("platform").is_some());
            assert!(loc.get("version").is_some());
            assert!(loc.get("state").is_some());
        }
        assert_eq!(locations[0]["platform"], "codex");
        assert_eq!(locations[1]["platform"], "claude");
    }

    #[test]
    fn doctor_json_showing_all() {
        let mut r = diverged_report_with_locations();
        r.rows.push(skill_row(
            "clipboard",
            "/u/.claude/skills",
            vec![Platform::Claude],
            ArtifactState::Tracked,
            Some("1.0.0"),
        ));
        r.artifacts.push(DoctorArtifact {
            kind: ArtifactKind::Skill,
            name: "clipboard".to_string(),
            scope: InstallScope::Global,
            state: ArtifactState::Tracked,
            version: Some("1.0.0".to_string()),
            versions: vec!["1.0.0".to_string()],
            tools: vec![Platform::Claude],
            source: Some("home".to_string()),
            locations: vec![PathBuf::from("/a")],
            diverged: false,
        });
        r.show_all = true;

        let value = doctor_json(&r);
        assert_eq!(value["showing"], "all");
        let artifacts = value["artifacts"].as_array().expect("artifacts array");
        assert_eq!(artifacts.len(), 2, "healthy tracked artifact included with --all: {value}");
        assert!(artifacts.iter().any(|a| a["name"] == "clipboard"));
    }
}
