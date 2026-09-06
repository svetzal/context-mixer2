//! Output rendering for `cmv check` and `cmv status`: the deterministic JSON
//! documents (`--json`) and the human listings. Neither carries a timestamp
//! or a temporary path, so the same inputs render byte-identical output. The
//! `Display` impls and their tests live here, beside each other, per the repo
//! convention.

use std::fmt;

use anyhow::{Context, Result};
use cmf::manifest::{KnowledgeBase, Manifest, ProfileRef};
use cmf::profile::Surface;
use serde::Serialize;

use crate::dispatch::StatusReport;
use crate::verdict::{IntentOutcome, State, Strictness, Summary, summarize};

/// Report schema version written in every JSON document's `schema` field.
pub const SCHEMA_VERSION: u32 = 1;

/// Whether to render for a person or for a machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// The linter-style listing or the status lines.
    Human,
    /// One pretty-printed JSON document.
    Json,
}

impl OutputFormat {
    /// Convert from the raw `--json` flag, exactly once, at the CLI boundary.
    pub fn from_flag(json: bool) -> Self {
        if json {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        }
    }
}

/// The full `cmv check` result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckReport {
    /// Report schema version.
    pub schema: u32,
    /// What the manifest said it compiled from.
    pub manifest: ManifestSummary,
    /// Languages the workspace was verified as.
    pub languages: Vec<String>,
    /// One outcome per compiled intent (manifest order), then per dropped
    /// intent.
    pub intents: Vec<IntentOutcome>,
    /// Counts, adherence, and the exit code.
    pub summary: Summary,
}

/// The manifest fields a report reader needs to identify the compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestSummary {
    /// The profile that drove selection.
    pub profile: ProfileRef,
    /// Where the records came from, as recorded at compile time.
    pub knowledge_base: KnowledgeBase,
}

impl CheckReport {
    /// Assemble the report and compute its summary.
    pub fn new(
        manifest: &Manifest,
        languages: &[String],
        intents: Vec<IntentOutcome>,
        strictness: Strictness,
    ) -> Self {
        let summary = summarize(&intents, strictness);
        Self {
            schema: SCHEMA_VERSION,
            manifest: ManifestSummary {
                profile: manifest.profile.clone(),
                knowledge_base: manifest.knowledge_base.clone(),
            },
            languages: languages.to_vec(),
            intents,
            summary,
        }
    }

    /// Render for `format`.
    pub fn render(&self, format: OutputFormat) -> Result<String> {
        match format {
            OutputFormat::Human => Ok(self.to_string()),
            OutputFormat::Json => to_json(self),
        }
    }
}

impl StatusReport {
    /// Render for `format`.
    pub fn render(&self, format: OutputFormat) -> Result<String> {
        match format {
            OutputFormat::Human => Ok(self.to_string()),
            OutputFormat::Json => to_json(self),
        }
    }
}

/// Pretty-printed JSON with a trailing newline.
fn to_json<T: Serialize>(value: &T) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value).context("could not serialize report")?;
    json.push('\n');
    Ok(json)
}

/// Width of the state label column; the key column starts two spaces later.
const LABEL_WIDTH: usize = 9;
const DETAIL_INDENT: usize = LABEL_WIDTH + 2;

/// The order states are listed in: settled outcomes first, problems last so
/// they sit beside the summary at the bottom of a terminal.
const LISTING_ORDER: [fn(&State) -> bool; 5] = [
    |state| matches!(state, State::Pass),
    |state| matches!(state, State::NotApplicable),
    |state| matches!(state, State::Unguided { .. }),
    |state| matches!(state, State::Unchecked { .. }),
    |state| matches!(state, State::Fail),
];

impl fmt::Display for CheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.intents.is_empty() {
            writeln!(f, "no intents in manifest")?;
        }
        for in_group in LISTING_ORDER {
            for intent in self.intents.iter().filter(|intent| in_group(&intent.state)) {
                write_intent(f, intent)?;
            }
        }
        writeln!(f)?;
        write_summary(f, &self.summary)?;
        let stale = self.intents.iter().filter(|intent| intent.stale).count();
        if stale > 0 {
            writeln!(
                f,
                "{stale} {} changed in the knowledge base since compile; re-run `cmf install` to recompile.",
                plural(stale, "record", "records")
            )?;
        }
        Ok(())
    }
}

fn write_intent(f: &mut fmt::Formatter<'_>, intent: &IntentOutcome) -> fmt::Result {
    write!(f, "{:<LABEL_WIDTH$}  {}", intent.state.label(), intent.key)?;
    if matches!(intent.state, State::Fail) {
        write!(
            f,
            "  ({})",
            if intent.required {
                "required"
            } else {
                "optional"
            }
        )?;
    }
    if intent.stale {
        write!(f, "  (stale: record changed since compile)")?;
    }
    writeln!(f)?;
    if let Some(reason) = intent.state.reason() {
        writeln!(f, "{:DETAIL_INDENT$}{reason}", "")?;
    }
    for evidence in &intent.evidence {
        writeln!(f, "{:DETAIL_INDENT$}{evidence}", "")?;
    }
    for location in &intent.locations {
        writeln!(f, "{:DETAIL_INDENT$}{}", "", location.render())?;
    }
    Ok(())
}

fn write_summary(f: &mut fmt::Formatter<'_>, summary: &Summary) -> fmt::Result {
    write!(
        f,
        "{} pass, {} fail, {} not applicable, {} unchecked, {} unguided; adherence ",
        summary.pass, summary.fail, summary.not_applicable, summary.unchecked, summary.unguided
    )?;
    match summary.adherence_rate {
        Some(rate) => writeln!(f, "{:.1}%", rate * 100.0),
        None => writeln!(f, "n/a"),
    }
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

impl fmt::Display for StatusReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Manifest: {}", self.manifest_path.display())?;
        writeln!(f, "Profile: {} {}", self.profile.id, self.profile.version)?;
        writeln!(f, "Artifact: {} ({})", self.artifact.name, surface_name(self.artifact.surface))?;
        writeln!(
            f,
            "Knowledge base: {} ({})",
            self.knowledge_base.path.display(),
            if self.knowledge_base.exists {
                "present"
            } else {
                "missing"
            }
        )?;
        writeln!(f, "Source: {}", self.knowledge_base.source.as_deref().unwrap_or("unregistered"))?;
        writeln!(
            f,
            "Pinned revision: {}",
            self.knowledge_base.revision.as_deref().unwrap_or("unavailable")
        )?;
        if self.languages.is_empty() {
            writeln!(f, "Languages: none detected")?;
        } else {
            writeln!(f, "Languages: {}", self.languages.join(", "))?;
        }
        writeln!(f, "Intents: {} compiled, {} dropped", self.intents, self.dropped)?;
        match &self.coverage {
            Some(coverage) => {
                writeln!(
                    f,
                    "Validators: {} of {} compiled {} have a validator for the detected languages",
                    coverage.with_validator,
                    self.intents,
                    plural(self.intents, "intent", "intents")
                )?;
                writeln!(f, "Stale records: {}", coverage.stale)?;
                writeln!(f, "Missing records: {}", coverage.missing)
            }
            None => writeln!(f, "Validators: unknown (knowledge base not scanned)"),
        }
    }
}

fn surface_name(surface: Surface) -> &'static str {
    match surface {
        Surface::Agent => "agent",
        Surface::Skill => "skill",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::{Coverage, KnowledgeBaseStatus};
    use crate::verdict::{Location, empty_object};
    use cmf::manifest::ArtifactRef;
    use serde_json::json;
    use std::path::PathBuf;

    fn manifest() -> Manifest {
        Manifest {
            schema: 1,
            compiled_at: "2026-09-05T14:02:11+00:00".to_string(),
            knowledge_base: KnowledgeBase {
                source: Some("guidelines".to_string()),
                path: PathBuf::from("/kb"),
                revision: Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".to_string()),
            },
            profile: ProfileRef {
                id: "rust-shipping".to_string(),
                version: "0.3.0".to_string(),
            },
            artifact: ArtifactRef {
                name: "AGENTS".to_string(),
                surface: Surface::Agent,
                checksum: "sha256:abc".to_string(),
            },
            intents: vec![],
            dropped: vec![],
        }
    }

    fn outcome(key: &str, state: State) -> IntentOutcome {
        IntentOutcome {
            id: Some(format!("kb.intent.{key}")),
            key: key.to_string(),
            language: Some("rust".to_string()),
            required: true,
            state,
            description: Some("Checked.".to_string()),
            signals: empty_object(),
            evidence: vec![],
            locations: vec![],
            stale: false,
        }
    }

    fn sample_outcomes() -> Vec<IntentOutcome> {
        let mut failing = outcome("rust/isolate-functional-core", State::Fail);
        failing.evidence = vec!["no gateway trait is declared".to_string()];
        failing.locations = vec![
            Location {
                path: "src/http.rs".to_string(),
                line: Some(14),
            },
            Location {
                path: "src/lib.rs".to_string(),
                line: None,
            },
        ];
        failing.signals = json!({ "effectful_modules": ["src/http.rs"] });
        let mut optional = outcome("rust/optional", State::Fail);
        optional.required = false;
        let mut unchecked = outcome(
            "python/type-public-boundaries",
            State::Unchecked {
                reason: "no validator for languages [rust]".to_string(),
            },
        );
        unchecked.stale = true;
        unchecked.language = None;
        unchecked.description = None;
        vec![
            failing,
            outcome("rust/put-gateways-at-effect-boundaries", State::Pass),
            unchecked,
            optional,
            outcome("rust/never-arises", State::NotApplicable),
            outcome(
                "rust/compile-public-documentation",
                State::Unguided {
                    reason: "budget".to_string(),
                },
            ),
        ]
    }

    fn report() -> CheckReport {
        CheckReport::new(&manifest(), &["rust".to_string()], sample_outcomes(), Strictness::Lenient)
    }

    #[test]
    fn human_listing_groups_by_state_with_problems_last() {
        let expected = "\
PASS       rust/put-gateways-at-effect-boundaries
N/A        rust/never-arises
UNGUIDED   rust/compile-public-documentation
           budget
UNCHECKED  python/type-public-boundaries  (stale: record changed since compile)
           no validator for languages [rust]
FAIL       rust/isolate-functional-core  (required)
           no gateway trait is declared
           src/http.rs:14
           src/lib.rs
FAIL       rust/optional  (optional)

1 pass, 2 fail, 1 not applicable, 1 unchecked, 1 unguided; adherence 33.3%
1 record changed in the knowledge base since compile; re-run `cmf install` to recompile.
";
        assert_eq!(report().render(OutputFormat::Human).unwrap(), expected);
    }

    #[test]
    fn human_listing_without_stale_records_has_no_remedy_line() {
        let report = CheckReport::new(
            &manifest(),
            &["rust".to_string()],
            vec![outcome("rust/a", State::Pass)],
            Strictness::Lenient,
        );
        assert_eq!(
            report.to_string(),
            "PASS       rust/a\n\n1 pass, 0 fail, 0 not applicable, 0 unchecked, 0 unguided; adherence 100.0%\n"
        );
    }

    #[test]
    fn human_listing_for_an_empty_manifest() {
        let report = CheckReport::new(&manifest(), &[], vec![], Strictness::Strict);
        assert_eq!(
            report.to_string(),
            "no intents in manifest\n\n0 pass, 0 fail, 0 not applicable, 0 unchecked, 0 unguided; adherence n/a\n"
        );
    }

    #[test]
    fn stale_remedy_line_pluralizes() {
        let mut outcomes = vec![
            outcome("rust/a", State::Pass),
            outcome("rust/b", State::Pass),
        ];
        outcomes[0].stale = true;
        outcomes[1].stale = true;
        let report = CheckReport::new(&manifest(), &[], outcomes, Strictness::Lenient);
        assert!(report.to_string().ends_with(
            "2 records changed in the knowledge base since compile; re-run `cmf install` to recompile.\n"
        ));
    }

    #[test]
    fn json_report_has_the_documented_shape_and_no_timestamp() {
        let json = report().render(OutputFormat::Json).unwrap();
        assert!(json.ends_with("}\n"));
        assert!(!json.contains("compiled_at"), "the report must not carry the compile timestamp");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["manifest"]["profile"]["id"], "rust-shipping");
        assert_eq!(value["manifest"]["knowledge_base"]["source"], "guidelines");
        assert_eq!(value["manifest"]["knowledge_base"]["path"], "/kb");
        assert_eq!(value["languages"], json!(["rust"]));
        assert_eq!(value["intents"].as_array().unwrap().len(), 6);
        assert_eq!(
            value["intents"][0],
            json!({
                "id": "kb.intent.rust/isolate-functional-core",
                "key": "rust/isolate-functional-core",
                "language": "rust",
                "required": true,
                "state": "fail",
                "description": "Checked.",
                "signals": { "effectful_modules": ["src/http.rs"] },
                "evidence": ["no gateway trait is declared"],
                "locations": [{ "path": "src/http.rs", "line": 14 }, { "path": "src/lib.rs" }],
                "stale": false,
            })
        );
        assert_eq!(value["intents"][2]["state"], "unchecked");
        assert_eq!(value["intents"][2]["reason"], "no validator for languages [rust]");
        assert_eq!(value["intents"][2]["language"], serde_json::Value::Null);
        assert_eq!(
            value["summary"],
            json!({
                "pass": 1,
                "fail": 2,
                "not_applicable": 1,
                "unchecked": 1,
                "unguided": 1,
                "adherence_rate": 0.3333,
                "exit_code": 1,
            })
        );
    }

    #[test]
    fn json_keeps_intent_order_and_puts_state_before_description() {
        let json = report().render(OutputFormat::Json).unwrap();
        let required = json.find("\"required\": true").unwrap();
        let state = json.find("\"state\": \"fail\"").unwrap();
        let description = json.find("\"description\": \"Checked.\"").unwrap();
        assert!(required < state && state < description, "field order drifted:\n{json}");
    }

    fn status_report() -> StatusReport {
        StatusReport {
            schema: 1,
            manifest_path: PathBuf::from("/project/.context-mixer/cmf-manifest.json"),
            profile: manifest().profile,
            artifact: manifest().artifact,
            knowledge_base: KnowledgeBaseStatus {
                path: PathBuf::from("/kb"),
                exists: true,
                source: Some("guidelines".to_string()),
                revision: Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".to_string()),
            },
            languages: vec!["python".to_string(), "rust".to_string()],
            intents: 4,
            dropped: 1,
            coverage: Some(Coverage {
                with_validator: 2,
                missing: 1,
                stale: 1,
            }),
        }
    }

    #[test]
    fn status_lines_name_every_fact() {
        let expected = "\
Manifest: /project/.context-mixer/cmf-manifest.json
Profile: rust-shipping 0.3.0
Artifact: AGENTS (agent)
Knowledge base: /kb (present)
Source: guidelines
Pinned revision: a1b2c3d4e5f60718293a4b5c6d7e8f9012345678
Languages: python, rust
Intents: 4 compiled, 1 dropped
Validators: 2 of 4 compiled intents have a validator for the detected languages
Stale records: 1
Missing records: 1
";
        assert_eq!(status_report().render(OutputFormat::Human).unwrap(), expected);
    }

    #[test]
    fn status_lines_spell_out_what_is_unavailable() {
        let mut report = status_report();
        report.knowledge_base = KnowledgeBaseStatus {
            path: PathBuf::from("/gone"),
            exists: false,
            source: None,
            revision: None,
        };
        report.languages = vec![];
        report.coverage = None;
        let text = report.to_string();
        assert!(text.contains("Knowledge base: /gone (missing)\n"), "{text}");
        assert!(text.contains("Source: unregistered\n"), "{text}");
        assert!(text.contains("Pinned revision: unavailable\n"), "{text}");
        assert!(text.contains("Languages: none detected\n"), "{text}");
        assert!(text.ends_with("Validators: unknown (knowledge base not scanned)\n"), "{text}");
    }

    #[test]
    fn status_json_carries_the_schema_and_coverage() {
        let json = status_report().render(OutputFormat::Json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["knowledge_base"]["exists"], true);
        assert_eq!(value["coverage"]["with_validator"], 2);
        assert_eq!(value["languages"], json!(["python", "rust"]));
    }
}
