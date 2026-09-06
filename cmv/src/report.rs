//! Output rendering for `cmv check`, `cmv status`, and `cmv explain`: the
//! deterministic JSON documents (`--json`) and the human listings. None
//! carries a timestamp or a temporary path, so the same inputs render
//! byte-identical output. The `Display` impls and their tests live here,
//! beside each other, per the repo convention.

use std::fmt;

use anyhow::{Context, Result};
use cmf::manifest::{KnowledgeBase, Manifest, ProfileRef};
use cmf::profile::Surface;
use serde::Serialize;

use crate::dispatch::{RecordResolution, StatusReport};
use crate::explain::{ExplainReport, ValidatorPlan};
use crate::pin::{KnowledgeBaseReport, VerifiedAgainst, short_revision};
use crate::resolve::ResolvedBy;
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
    /// Where cmv actually read the records from, and which tree it verified.
    pub knowledge_base: KnowledgeBaseReport,
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
        knowledge_base: KnowledgeBaseReport,
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
            knowledge_base,
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

impl ExplainReport {
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
        write_remedy(f, &self.knowledge_base, stale)
    }
}

/// The one remedy line, when there is something to remedy: the knowledge base
/// has moved past the pin (with how much of the compile that touched), or,
/// short of that, some compiled records changed. Informational either way;
/// cmv never re-pins.
fn write_remedy(
    f: &mut fmt::Formatter<'_>,
    knowledge_base: &KnowledgeBaseReport,
    stale: usize,
) -> fmt::Result {
    if knowledge_base.moved {
        return write_moved_line(f, knowledge_base, Some(stale));
    }
    if stale > 0 {
        writeln!(
            f,
            "{stale} {} changed in the knowledge base since compile; re-run `cmf install` to recompile.",
            plural(stale, "record", "records")
        )?;
    }
    Ok(())
}

/// `knowledge base has moved: HEAD <short> vs pinned <short>; N compiled
/// records or validators changed; re-run cmf install to recompile`. The count
/// is omitted when the knowledge base could not be scanned.
fn write_moved_line(
    f: &mut fmt::Formatter<'_>,
    knowledge_base: &KnowledgeBaseReport,
    changed: Option<usize>,
) -> fmt::Result {
    let head = knowledge_base.head_revision.as_deref().map_or("unknown", short_revision);
    let pinned = knowledge_base.pinned_revision.as_deref().map_or("unknown", short_revision);
    write!(f, "knowledge base has moved: HEAD {head} vs pinned {pinned}")?;
    if let Some(changed) = changed {
        write!(
            f,
            "; {changed} compiled {} changed",
            plural(changed, "record or validator", "records or validators")
        )?;
    }
    writeln!(f, "; re-run cmf install to recompile")
}

fn resolved_by_name(resolved_by: ResolvedBy) -> &'static str {
    match resolved_by {
        ResolvedBy::Override => "--knowledge-base",
        ResolvedBy::Source => "cmx source",
        ResolvedBy::Path => "manifest path",
    }
}

fn verified_against_name(verified_against: VerifiedAgainst) -> &'static str {
    match verified_against {
        VerifiedAgainst::Pinned => "pinned revision",
        VerifiedAgainst::Head => "working tree (HEAD)",
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
        let kb = &self.knowledge_base.resolved;
        writeln!(
            f,
            "Knowledge base: {} ({}, resolved by {})",
            kb.path.display(),
            if self.knowledge_base.exists {
                "present"
            } else {
                "missing"
            },
            resolved_by_name(kb.resolved_by)
        )?;
        writeln!(f, "Source: {}", kb.source.as_deref().unwrap_or("unregistered"))?;
        writeln!(f, "Pinned revision: {}", kb.pinned_revision.as_deref().unwrap_or("unavailable"))?;
        writeln!(f, "HEAD revision: {}", kb.head_revision.as_deref().unwrap_or("unavailable"))?;
        writeln!(f, "Verified against: {}", verified_against_name(kb.verified_against))?;
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
                writeln!(f, "Missing records: {}", coverage.missing)?;
            }
            None => writeln!(f, "Validators: unknown (knowledge base not scanned)")?,
        }
        if kb.moved {
            write_moved_line(f, kb, self.coverage.as_ref().map(|coverage| coverage.stale))?;
        }
        Ok(())
    }
}

impl fmt::Display for ExplainReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let intent = &self.intent;
        writeln!(f, "Intent: {}", intent.key)?;
        writeln!(f, "Id: {}", intent.id.as_deref().unwrap_or("unknown"))?;
        match (intent.resolution, &intent.resolution_detail) {
            (RecordResolution::Key, _) => writeln!(f, "Record: resolved by key")?,
            (RecordResolution::Id, _) => writeln!(
                f,
                "Record: resolved by id (no record at the manifest key; one record carries its id)"
            )?,
            (RecordResolution::NotFound, None) => {
                writeln!(f, "Record: not found in knowledge base")?;
            }
            (RecordResolution::NotFound, Some(detail)) => {
                writeln!(f, "Record: not found in knowledge base ({detail})")?;
            }
        }
        if let Some(title) = &intent.title {
            writeln!(f, "Title: {title}")?;
        }
        if let Some(status) = &intent.status {
            writeln!(f, "Status: {status} (informational; never gates)")?;
        }
        writeln!(f, "Compiled: {}", yes_no(intent.compiled))?;
        match &intent.drop_reason {
            Some(reason) => {
                writeln!(f, "Dropped: yes ({reason}); the guidance never reached the artifact")?;
            }
            None => writeln!(f, "Dropped: no")?,
        }
        writeln!(
            f,
            "Stale: {}",
            if intent.stale {
                "yes (record or validator changed at HEAD since compile)"
            } else {
                "no"
            }
        )?;
        let kb = &self.knowledge_base;
        writeln!(
            f,
            "Knowledge base: {} (resolved by {}, verified against {})",
            kb.path.display(),
            resolved_by_name(kb.resolved_by),
            verified_against_name(kb.verified_against)
        )?;
        if self.languages.is_empty() {
            writeln!(f, "Languages: none detected")?;
        } else {
            writeln!(f, "Languages: {}", self.languages.join(", "))?;
        }
        writeln!(f, "Config: {}", intent.config)?;
        if intent.validators.is_empty() {
            if intent.resolution == RecordResolution::NotFound {
                writeln!(f, "Validators: unknown (record not in knowledge base)")?;
            } else {
                writeln!(f, "Validators: none declared")?;
            }
        } else {
            writeln!(f, "Validators:")?;
            for plan in &intent.validators {
                write_plan(f, plan)?;
            }
        }
        if kb.moved {
            write_moved_line(f, kb, None)?;
        }
        Ok(())
    }
}

fn write_plan(f: &mut fmt::Formatter<'_>, plan: &ValidatorPlan) -> fmt::Result {
    write!(
        f,
        "  {}  {}  ({})  ",
        plan.language,
        plan.run,
        if plan.required {
            "required"
        } else {
            "optional"
        }
    )?;
    match &plan.skipped {
        Some(reason) => writeln!(f, "skipped: {reason}")?,
        None => writeln!(f, "would run")?,
    }
    writeln!(f, "    {}", plan.description)?;
    if let Some(argv) = &plan.argv {
        writeln!(f, "    {}", argv.join(" "))?;
    }
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
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
    use crate::explain::IntentExplanation;
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

    const PIN: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const HEAD: &str = "ffffffffffffffffffffffffffffffffffffffff";

    fn knowledge_base() -> KnowledgeBaseReport {
        KnowledgeBaseReport {
            path: PathBuf::from("/kb"),
            resolved_by: ResolvedBy::Source,
            source: Some("guidelines".to_string()),
            pinned_revision: Some(PIN.to_string()),
            head_revision: Some(PIN.to_string()),
            verified_against: VerifiedAgainst::Pinned,
            moved: false,
        }
    }

    fn moved_knowledge_base() -> KnowledgeBaseReport {
        KnowledgeBaseReport {
            head_revision: Some(HEAD.to_string()),
            moved: true,
            ..knowledge_base()
        }
    }

    fn check_report(
        knowledge_base: KnowledgeBaseReport,
        outcomes: Vec<IntentOutcome>,
        strictness: Strictness,
    ) -> CheckReport {
        CheckReport::new(&manifest(), knowledge_base, &["rust".to_string()], outcomes, strictness)
    }

    fn report() -> CheckReport {
        check_report(knowledge_base(), sample_outcomes(), Strictness::Lenient)
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
        let report = check_report(
            knowledge_base(),
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
        let report =
            CheckReport::new(&manifest(), knowledge_base(), &[], vec![], Strictness::Strict);
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
        let report = check_report(knowledge_base(), outcomes, Strictness::Lenient);
        assert!(report.to_string().ends_with(
            "2 records changed in the knowledge base since compile; re-run `cmf install` to recompile.\n"
        ));
    }

    #[test]
    fn moved_knowledge_base_replaces_the_stale_line_with_the_moved_line() {
        let mut outcomes = vec![
            outcome("rust/a", State::Pass),
            outcome("rust/b", State::Pass),
        ];
        outcomes[0].stale = true;
        let text = check_report(moved_knowledge_base(), outcomes, Strictness::Lenient).to_string();
        assert!(
            text.ends_with(
                "2 pass, 0 fail, 0 not applicable, 0 unchecked, 0 unguided; adherence 100.0%\n\
                 knowledge base has moved: HEAD ffffffffffff vs pinned a1b2c3d4e5f6; 1 compiled record or validator changed; re-run cmf install to recompile\n"
            ),
            "{text}"
        );
        assert!(!text.contains("changed in the knowledge base since compile"), "{text}");
    }

    #[test]
    fn moved_line_pluralizes_and_reports_zero_changes() {
        let text = check_report(
            moved_knowledge_base(),
            vec![outcome("rust/a", State::Pass)],
            Strictness::Lenient,
        )
        .to_string();
        assert!(text.contains("; 0 compiled records or validators changed; "), "{text}");
    }

    #[test]
    fn moved_line_never_changes_the_exit_code() {
        let report = check_report(
            moved_knowledge_base(),
            vec![outcome("rust/a", State::Pass)],
            Strictness::Strict,
        );
        assert_eq!(report.summary.exit_code, 0);
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
        assert_eq!(
            value["knowledge_base"],
            json!({
                "path": "/kb",
                "resolved_by": "source",
                "source": "guidelines",
                "pinned_revision": PIN,
                "head_revision": PIN,
                "verified_against": "pinned",
                "moved": false,
            })
        );
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
                resolved: knowledge_base(),
                exists: true,
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
Knowledge base: /kb (present, resolved by cmx source)
Source: guidelines
Pinned revision: a1b2c3d4e5f60718293a4b5c6d7e8f9012345678
HEAD revision: a1b2c3d4e5f60718293a4b5c6d7e8f9012345678
Verified against: pinned revision
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
            resolved: KnowledgeBaseReport {
                path: PathBuf::from("/gone"),
                resolved_by: ResolvedBy::Path,
                source: None,
                pinned_revision: None,
                head_revision: None,
                verified_against: VerifiedAgainst::Head,
                moved: false,
            },
            exists: false,
        };
        report.languages = vec![];
        report.coverage = None;
        let text = report.to_string();
        assert!(
            text.contains("Knowledge base: /gone (missing, resolved by manifest path)\n"),
            "{text}"
        );
        assert!(text.contains("Source: unregistered\n"), "{text}");
        assert!(text.contains("Pinned revision: unavailable\n"), "{text}");
        assert!(text.contains("HEAD revision: unavailable\n"), "{text}");
        assert!(text.contains("Verified against: working tree (HEAD)\n"), "{text}");
        assert!(text.contains("Languages: none detected\n"), "{text}");
        assert!(text.ends_with("Validators: unknown (knowledge base not scanned)\n"), "{text}");
    }

    #[test]
    fn status_reports_a_moved_knowledge_base_with_the_stale_count() {
        let mut report = status_report();
        report.knowledge_base.resolved = moved_knowledge_base();
        let text = report.to_string();
        assert!(
            text.ends_with(
                "Missing records: 1\nknowledge base has moved: HEAD ffffffffffff vs pinned a1b2c3d4e5f6; 1 compiled record or validator changed; re-run cmf install to recompile\n"
            ),
            "{text}"
        );
        report.coverage = None;
        let text = report.to_string();
        assert!(
            text.ends_with(
                "knowledge base has moved: HEAD ffffffffffff vs pinned a1b2c3d4e5f6; re-run cmf install to recompile\n"
            ),
            "the count is omitted without coverage: {text}"
        );
    }

    #[test]
    fn status_json_carries_the_schema_and_coverage() {
        let json = status_report().render(OutputFormat::Json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["knowledge_base"]["exists"], true);
        assert_eq!(value["knowledge_base"]["resolved_by"], "source");
        assert_eq!(value["knowledge_base"]["verified_against"], "pinned");
        assert_eq!(value["knowledge_base"]["moved"], false);
        assert_eq!(value["coverage"]["with_validator"], 2);
        assert_eq!(value["languages"], json!(["python", "rust"]));
    }

    fn explain_report() -> ExplainReport {
        ExplainReport {
            schema: 1,
            knowledge_base: knowledge_base(),
            languages: vec!["rust".to_string()],
            intent: IntentExplanation {
                key: "rust/isolate-functional-core".to_string(),
                id: Some("kb.intent.isolate-functional-core".to_string()),
                resolution: RecordResolution::Key,
                resolution_detail: None,
                title: Some("Isolate the functional core".to_string()),
                status: Some("confirmed".to_string()),
                compiled: true,
                dropped: false,
                drop_reason: None,
                stale: true,
                config: json!({ "business_rule_minimum_matches": 2 }),
                validators: vec![
                    ValidatorPlan {
                        language: "rust".to_string(),
                        run: "checks/rust/isolate.sh".to_string(),
                        required: true,
                        description: "No rules beside I/O.".to_string(),
                        would_run: true,
                        skipped: None,
                        argv: Some(
                            [
                                "/kb/checks/rust/isolate.sh",
                                "--workspace",
                                "/project",
                                "--config",
                                "<scratch>/1.json",
                            ]
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                        ),
                    },
                    ValidatorPlan {
                        language: "python".to_string(),
                        run: "checks/python/isolate.py".to_string(),
                        required: false,
                        description: "No rules beside clients.".to_string(),
                        would_run: false,
                        skipped: Some(
                            "language python is not among the workspace's [rust]".to_string(),
                        ),
                        argv: Some(
                            [
                                "/kb/checks/python/isolate.py",
                                "--workspace",
                                "/project",
                                "--config",
                                "<scratch>/1.json",
                            ]
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                        ),
                    },
                ],
            },
        }
    }

    #[test]
    fn explain_lines_name_every_fact() {
        let expected = "\
Intent: rust/isolate-functional-core
Id: kb.intent.isolate-functional-core
Record: resolved by key
Title: Isolate the functional core
Status: confirmed (informational; never gates)
Compiled: yes
Dropped: no
Stale: yes (record or validator changed at HEAD since compile)
Knowledge base: /kb (resolved by cmx source, verified against pinned revision)
Languages: rust
Config: {\"business_rule_minimum_matches\":2}
Validators:
  rust  checks/rust/isolate.sh  (required)  would run
    No rules beside I/O.
    /kb/checks/rust/isolate.sh --workspace /project --config <scratch>/1.json
  python  checks/python/isolate.py  (optional)  skipped: language python is not among the workspace's [rust]
    No rules beside clients.
    /kb/checks/python/isolate.py --workspace /project --config <scratch>/1.json
";
        assert_eq!(explain_report().render(OutputFormat::Human).unwrap(), expected);
    }

    #[test]
    fn explain_lines_for_a_dropped_intent_whose_record_is_gone() {
        let mut report = explain_report();
        report.knowledge_base = moved_knowledge_base();
        report.intent = IntentExplanation {
            key: "rust/budgeted".to_string(),
            id: None,
            resolution: RecordResolution::NotFound,
            resolution_detail: None,
            title: None,
            status: None,
            compiled: false,
            dropped: true,
            drop_reason: Some("budget".to_string()),
            stale: false,
            config: json!({}),
            validators: vec![],
        };
        let text = report.to_string();
        assert!(text.contains("Id: unknown\n"), "{text}");
        assert!(text.contains("Record: not found in knowledge base\n"), "{text}");
        assert!(!text.contains("Title:"), "{text}");
        assert!(text.contains("Compiled: no\n"), "{text}");
        assert!(
            text.contains("Dropped: yes (budget); the guidance never reached the artifact\n"),
            "{text}"
        );
        assert!(text.contains("Stale: no\n"), "{text}");
        assert!(text.contains("Config: {}\n"), "{text}");
        assert!(text.contains("Validators: unknown (record not in knowledge base)\n"), "{text}");
        assert!(
            text.ends_with(
                "knowledge base has moved: HEAD ffffffffffff vs pinned a1b2c3d4e5f6; re-run cmf install to recompile\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn explain_names_a_moved_record_and_the_records_sharing_a_lost_key_id() {
        let mut report = explain_report();
        report.intent.resolution = RecordResolution::Id;
        assert!(
            report.to_string().contains(
                "Record: resolved by id (no record at the manifest key; one record carries its id)\n"
            ),
            "{report}"
        );

        report.intent.resolution = RecordResolution::NotFound;
        report.intent.resolution_detail = Some(
            "no record at key rust/gone, and id kb.intent.a is shared by 2 records (python/a, rust/a); re-run cmf install to recompile".to_string(),
        );
        report.intent.validators.clear();
        let text = report.to_string();
        assert!(
            text.contains(
                "Record: not found in knowledge base (no record at key rust/gone, and id kb.intent.a is shared by 2 records (python/a, rust/a); re-run cmf install to recompile)\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn explain_json_carries_the_schema_and_the_knowledge_base_block() {
        let json = explain_report().render(OutputFormat::Json).unwrap();
        assert!(json.ends_with("}\n"));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["knowledge_base"]["resolved_by"], "source");
        assert_eq!(value["intent"]["resolution"], "key");
        assert_eq!(value["intent"]["validators"][1]["would_run"], false);
        assert_eq!(value["intent"]["config"], json!({ "business_rule_minimum_matches": 2 }));
    }
}
