//! The verification core: [`check`] resolves every compiled intent against the
//! knowledge base, runs the validators that match the workspace's languages,
//! and maps each run onto exactly one [`IntentOutcome`]; [`status`] answers
//! the same resolution questions without running anything. Pure over the
//! `Filesystem` and [`ProcessRunner`] gateways: the same inputs against the
//! in-memory fakes produce the same outcomes as against the OS.
//!
//! Resolution is by record `id` first, then by catalog `key`, so a record
//! that moved in the knowledge base is still found. A record whose bytes no
//! longer match the manifest checksum is reported `stale`, which never changes
//! the exit code: the remedy is `cmf install`, because a newer knowledge base
//! may change selection, and that is cmf's decision.
//!
//! When several validators match (a record may declare one per language, and
//! a workspace may have several languages) they combine **all-must-pass**: any
//! failure fails the intent, otherwise any unchecked run leaves it unchecked,
//! otherwise it passes when at least one run was applicable. `CMV.md` lists
//! the alternative (any-passes) as an open decision; all-must-pass is the
//! conservative reading and the one implemented here.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use cmf::catalog::{Intent, Validator};
use cmf::manifest::{ArtifactRef, DroppedIntent, IntentRef, Manifest, ProfileRef};
use cmx_core::checksum;
use cmx_core::gateway::Filesystem;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::ProjectConfig;
use crate::process::{ProcessOutcome, ProcessRequest, ProcessRunner};
use crate::verdict::{self, IntentOutcome, Location, State, empty_object};

/// The scanned knowledge base, keyed by catalog key.
pub type Catalog = BTreeMap<String, Intent>;

/// Everything [`check`] needs besides its gateways.
pub struct CheckRequest<'a> {
    /// The manifest cmf compiled for the workspace.
    pub manifest: &'a Manifest,
    /// The knowledge base as scanned by `cmf::catalog::scan`.
    pub catalog: &'a Catalog,
    /// The project's `cmv.toml`.
    pub config: &'a ProjectConfig,
    /// Languages to verify as; validators for other languages do not run.
    pub languages: &'a [String],
    /// Knowledge-base root. It is canonicalized through the filesystem
    /// gateway before use: validator `run` paths resolve against it and it is
    /// the validators' working directory.
    pub knowledge_base: &'a Path,
    /// The project root handed to validators as `--workspace`.
    pub workspace: &'a Path,
    /// Directory where each intent's `--config` JSON is written. The caller
    /// owns its lifetime; nothing here appears in the report.
    pub scratch: &'a Path,
}

/// Verify every intent in the manifest, in manifest order, then report each
/// dropped intent as unguided.
pub fn check(
    request: &CheckRequest<'_>,
    fs: &dyn Filesystem,
    runner: &dyn ProcessRunner,
) -> Result<Vec<IntentOutcome>> {
    let resolver = Resolver::new(request.catalog);
    // Validators run with the knowledge base as their working directory, and a
    // relative program path would be looked up against that new directory
    // rather than cmv's; an absolute root makes `<root>/<run>` unambiguous.
    let knowledge_base = fs.canonicalize(request.knowledge_base).with_context(|| {
        format!("could not resolve knowledge base {}", request.knowledge_base.display())
    })?;
    let mut outcomes =
        Vec::with_capacity(request.manifest.intents.len() + request.manifest.dropped.len());
    for (index, entry) in request.manifest.intents.iter().enumerate() {
        outcomes.push(check_intent(index, entry, request, &knowledge_base, &resolver, fs, runner)?);
    }
    for dropped in &request.manifest.dropped {
        outcomes.push(unguided(dropped, &resolver));
    }
    Ok(outcomes)
}

fn check_intent(
    index: usize,
    entry: &IntentRef,
    request: &CheckRequest<'_>,
    knowledge_base: &Path,
    resolver: &Resolver<'_>,
    fs: &dyn Filesystem,
    runner: &dyn ProcessRunner,
) -> Result<IntentOutcome> {
    let Some(intent) = resolver.resolve(entry) else {
        return Ok(unchecked(entry, "record not in knowledge base", false));
    };
    let stale = is_stale(intent, entry, fs)?;
    let validators: Vec<Validator<'_>> = intent
        .record
        .validators()
        .filter(|validator| request.languages.iter().any(|language| language == validator.language))
        .collect();
    if validators.is_empty() {
        let reason = format!("no validator for languages [{}]", request.languages.join(", "));
        return Ok(unchecked(entry, &reason, stale));
    }
    let config_path = request.scratch.join(format!("{index}.json"));
    let config_json = serde_json::to_string(&request.config.intent_config(&entry.key))
        .context("could not serialize intent config")?;
    fs.write(&config_path, &config_json)
        .with_context(|| format!("could not write validator config {}", config_path.display()))?;
    let runs: Vec<ValidatorRun> = validators
        .iter()
        .map(|validator| run_validator(validator, knowledge_base, &config_path, request, runner))
        .collect();
    Ok(combine(entry, &runs, stale))
}

/// Whether the record's bytes no longer match what the manifest recorded.
fn is_stale(intent: &Intent, entry: &IntentRef, fs: &dyn Filesystem) -> Result<bool> {
    let current = checksum::checksum_file(&intent.path, fs)
        .with_context(|| format!("could not checksum record {}", intent.path.display()))?;
    Ok(current != entry.checksum)
}

/// One validator's contribution to an intent's outcome.
struct ValidatorRun {
    language: String,
    required: bool,
    description: String,
    state: State,
    signals: Value,
    evidence: Vec<String>,
    locations: Vec<Location>,
}

fn run_validator(
    validator: &Validator<'_>,
    knowledge_base: &Path,
    config_path: &Path,
    request: &CheckRequest<'_>,
    runner: &dyn ProcessRunner,
) -> ValidatorRun {
    let timeout = request.config.validator_timeout();
    let process = ProcessRequest {
        program: knowledge_base.join(validator.run),
        args: vec![
            OsString::from("--workspace"),
            request.workspace.as_os_str().to_owned(),
            OsString::from("--config"),
            config_path.as_os_str().to_owned(),
        ],
        cwd: knowledge_base.to_path_buf(),
        timeout,
    };
    let (state, signals, evidence, locations) =
        match interpret(runner.run(&process), validator.run, timeout) {
            Ok(verdict) => (verdict.state(), verdict.signals, verdict.evidence, verdict.locations),
            Err(reason) => (State::Unchecked { reason }, empty_object(), vec![], vec![]),
        };
    ValidatorRun {
        language: validator.language.to_string(),
        required: validator.required,
        description: validator.description.to_string(),
        state,
        signals,
        evidence,
        locations,
    }
}

/// Turn how the process ended into a verdict, or the reason there is none.
fn interpret(
    outcome: ProcessOutcome,
    run: &Path,
    timeout: Duration,
) -> std::result::Result<verdict::Verdict, String> {
    match outcome {
        ProcessOutcome::FailedToStart { reason } => {
            Err(format!("could not start {}: {reason}", run.display()))
        }
        ProcessOutcome::TimedOut => Err(format!("timed out after {}s", timeout.as_secs())),
        ProcessOutcome::Exited {
            code: Some(0),
            stdout,
            ..
        } => verdict::parse(&stdout).map_err(|error| format!("invalid verdict JSON: {error}")),
        ProcessOutcome::Exited { code, stderr, .. } => {
            let status = code
                .map_or_else(|| "killed by signal".to_string(), |code| format!("exited {code}"));
            Err(match stderr.lines().map(str::trim).find(|line| !line.is_empty()) {
                Some(line) => format!("{status}: {line}"),
                None => status,
            })
        }
    }
}

/// Fold several validator runs into one outcome under the all-must-pass rule
/// described in the module docs.
///
/// `required`, and the reason of an unchecked outcome, come from the runs that
/// produced the chosen state — so an optional validator's failure beside a
/// required validator's pass reports an optional failure, not a required one.
/// `signals` is the single run's value verbatim, or an object keyed by
/// language when several ran.
fn combine(entry: &IntentRef, runs: &[ValidatorRun], stale: bool) -> IntentOutcome {
    let verdict_state = if runs.iter().any(|run| matches!(run.state, State::Fail)) {
        State::Fail
    } else if runs.iter().any(|run| matches!(run.state, State::Unchecked { .. })) {
        State::Unchecked {
            reason: unchecked_reasons(runs),
        }
    } else if runs.iter().any(|run| matches!(run.state, State::Pass)) {
        State::Pass
    } else {
        State::NotApplicable
    };
    let contributing = |run: &&ValidatorRun| {
        std::mem::discriminant(&run.state) == std::mem::discriminant(&verdict_state)
    };
    let required = runs.iter().filter(contributing).any(|run| run.required);
    let mut languages: Vec<&str> = runs.iter().map(|run| run.language.as_str()).collect();
    languages.sort_unstable();
    languages.dedup();
    let mut descriptions: Vec<&str> = Vec::new();
    for run in runs {
        if !descriptions.contains(&run.description.as_str()) {
            descriptions.push(&run.description);
        }
    }
    let signals = match runs {
        [single] => single.signals.clone(),
        several => Value::Object(
            several
                .iter()
                .map(|run| (run.language.clone(), run.signals.clone()))
                .collect::<Map<_, _>>(),
        ),
    };
    IntentOutcome {
        id: Some(entry.id.clone()),
        key: entry.key.clone(),
        language: Some(languages.join(", ")),
        required,
        state: verdict_state,
        description: Some(descriptions.join(" / ")),
        signals,
        evidence: runs.iter().flat_map(|run| run.evidence.iter().cloned()).collect(),
        locations: runs.iter().flat_map(|run| run.locations.iter().cloned()).collect(),
        stale,
    }
}

/// The unchecked runs' reasons, prefixed by language when more than one
/// validator ran so the reader can tell which one produced which.
fn unchecked_reasons(runs: &[ValidatorRun]) -> String {
    let unchecked: Vec<String> = runs
        .iter()
        .filter_map(|run| run.state.reason().map(|reason| (run.language.as_str(), reason)))
        .map(|(language, reason)| {
            if runs.len() > 1 {
                format!("{language}: {reason}")
            } else {
                reason.to_string()
            }
        })
        .collect();
    unchecked.join("; ")
}

fn unchecked(entry: &IntentRef, reason: &str, stale: bool) -> IntentOutcome {
    IntentOutcome {
        id: Some(entry.id.clone()),
        key: entry.key.clone(),
        language: None,
        required: false,
        state: State::Unchecked {
            reason: reason.to_string(),
        },
        description: None,
        signals: empty_object(),
        evidence: vec![],
        locations: vec![],
        stale,
    }
}

fn unguided(dropped: &DroppedIntent, resolver: &Resolver<'_>) -> IntentOutcome {
    IntentOutcome {
        id: resolver.by_key(&dropped.key).map(|intent| intent.record.id.clone()),
        key: dropped.key.clone(),
        language: None,
        required: false,
        state: State::Unguided {
            reason: dropped.reason.clone(),
        },
        description: None,
        signals: empty_object(),
        evidence: vec![],
        locations: vec![],
        stale: false,
    }
}

/// Looks records up by `id` first, then by `key`.
struct Resolver<'a> {
    catalog: &'a Catalog,
    by_id: BTreeMap<&'a str, &'a Intent>,
}

impl<'a> Resolver<'a> {
    fn new(catalog: &'a Catalog) -> Self {
        let mut by_id = BTreeMap::new();
        // Catalog order is key order, so on a duplicated id the first key wins
        // deterministically.
        for intent in catalog.values() {
            by_id.entry(intent.record.id.as_str()).or_insert(intent);
        }
        Self { catalog, by_id }
    }

    fn resolve(&self, entry: &IntentRef) -> Option<&'a Intent> {
        self.by_id.get(entry.id.as_str()).copied().or_else(|| self.by_key(&entry.key))
    }

    fn by_key(&self, key: &str) -> Option<&'a Intent> {
        self.catalog.get(key)
    }
}

/// What `cmv status` reports: the manifest's identity, the knowledge base's
/// whereabouts and pin, the workspace's languages, and how much of the
/// manifest the knowledge base can currently vouch for. Nothing is executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// Report schema version.
    pub schema: u32,
    /// The manifest that was read.
    pub manifest_path: PathBuf,
    /// The profile that drove selection.
    pub profile: ProfileRef,
    /// The delivered artifact.
    pub artifact: ArtifactRef,
    /// The knowledge base as cmv resolved it.
    pub knowledge_base: KnowledgeBaseStatus,
    /// Languages the workspace verifies as.
    pub languages: Vec<String>,
    /// Compiled intents in the manifest.
    pub intents: usize,
    /// Dropped intents in the manifest.
    pub dropped: usize,
    /// How the knowledge base covers the manifest; `None` when it could not be
    /// scanned.
    pub coverage: Option<Coverage>,
}

/// Where the knowledge base is and what the manifest pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeBaseStatus {
    /// The root cmv resolved (the manifest's path unless overridden).
    pub path: PathBuf,
    /// Whether that root is a directory.
    pub exists: bool,
    /// The cmx source name the manifest recorded, if any.
    pub source: Option<String>,
    /// The git revision the manifest recorded, if any.
    pub revision: Option<String>,
}

/// How the scanned knowledge base covers the manifest's compiled intents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// Compiled intents with at least one validator for the workspace's
    /// languages.
    pub with_validator: usize,
    /// Compiled intents whose record is no longer in the knowledge base.
    pub missing: usize,
    /// Compiled intents whose record changed since compile.
    pub stale: usize,
}

/// Summarize the manifest against the knowledge base without running
/// validators. `catalog` is `None` when the knowledge base could not be
/// scanned; the report then says where it looked and stops short of coverage.
pub fn status(
    manifest: &Manifest,
    manifest_path: &Path,
    catalog: Option<&Catalog>,
    languages: &[String],
    knowledge_base: &Path,
    fs: &dyn Filesystem,
) -> Result<StatusReport> {
    let coverage = match catalog {
        Some(catalog) => Some(coverage(manifest, catalog, languages, fs)?),
        None => None,
    };
    Ok(StatusReport {
        schema: crate::report::SCHEMA_VERSION,
        manifest_path: manifest_path.to_path_buf(),
        profile: manifest.profile.clone(),
        artifact: manifest.artifact.clone(),
        knowledge_base: KnowledgeBaseStatus {
            path: knowledge_base.to_path_buf(),
            exists: fs.is_dir(knowledge_base),
            source: manifest.knowledge_base.source.clone(),
            revision: manifest.knowledge_base.revision.clone(),
        },
        languages: languages.to_vec(),
        intents: manifest.intents.len(),
        dropped: manifest.dropped.len(),
        coverage,
    })
}

fn coverage(
    manifest: &Manifest,
    catalog: &Catalog,
    languages: &[String],
    fs: &dyn Filesystem,
) -> Result<Coverage> {
    let resolver = Resolver::new(catalog);
    let mut coverage = Coverage {
        with_validator: 0,
        missing: 0,
        stale: 0,
    };
    for entry in &manifest.intents {
        let Some(intent) = resolver.resolve(entry) else {
            coverage.missing += 1;
            continue;
        };
        if is_stale(intent, entry, fs)? {
            coverage.stale += 1;
        }
        if intent
            .record
            .validators()
            .any(|validator| languages.iter().any(|language| language == validator.language))
        {
            coverage.with_validator += 1;
        }
    }
    Ok(coverage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{FakeProcessRunner, exited};
    use cmf::manifest::KnowledgeBase;
    use cmf::profile::Surface;
    use cmx_core::gateway::fakes::FakeFilesystem;
    use serde_json::json;

    const KB: &str = "/kb";
    const WORKSPACE: &str = "/project";
    const SCRATCH: &str = "/scratch";

    fn record(id: &str, evidence: &[String]) -> String {
        format!(
            r#"
id = "{id}"
title = "Title"
category = "quality"
status = "confirmed"
capability = "c"
threat = "t"
expectation = "e"
strategy = "s"
tradeoff = "o"
evidence = [
  {}
]
"#,
            evidence.join(",\n  ")
        )
    }

    fn check_entry(kind: &str, language: &str, run: &str, required: bool) -> String {
        format!(
            r#"{{ type = "{kind}", language = "{language}", run = "{run}", description = "{language} check", required = {required} }}"#
        )
    }

    /// A knowledge base on a fake filesystem plus the manifest entries that
    /// point at it, built up by the tests.
    struct Kb {
        fs: FakeFilesystem,
        intents: Vec<IntentRef>,
        dropped: Vec<DroppedIntent>,
    }

    impl Kb {
        fn new() -> Self {
            let fs = FakeFilesystem::new();
            fs.add_dir(format!("{KB}/intents"));
            fs.add_dir(SCRATCH);
            Self {
                fs,
                intents: vec![],
                dropped: vec![],
            }
        }

        /// Add a record and a manifest entry whose checksum matches it.
        fn add(&mut self, key: &str, evidence: &[String]) -> &mut Self {
            let id = format!("kb.intent.{}", key.rsplit('/').next().unwrap());
            let body = record(&id, evidence);
            self.fs.add_file(format!("{KB}/intents/{key}.toml"), body.as_bytes());
            self.intents.push(IntentRef {
                id,
                key: key.to_string(),
                checksum: checksum::checksum_bytes(body.as_bytes()),
            });
            self
        }

        /// Add a manifest entry with no record behind it.
        fn add_missing(&mut self, key: &str) -> &mut Self {
            self.intents.push(IntentRef {
                id: format!("kb.intent.{key}"),
                key: key.to_string(),
                checksum: "sha256:0000".to_string(),
            });
            self
        }

        fn drop_key(&mut self, key: &str, reason: &str) -> &mut Self {
            self.dropped.push(DroppedIntent {
                key: key.to_string(),
                reason: reason.to_string(),
            });
            self
        }

        fn manifest(&self) -> Manifest {
            Manifest {
                schema: 1,
                compiled_at: "2026-09-05T14:02:11+00:00".to_string(),
                knowledge_base: KnowledgeBase {
                    source: Some("guidelines".to_string()),
                    path: PathBuf::from(KB),
                    revision: None,
                },
                profile: ProfileRef {
                    id: "shipping".to_string(),
                    version: "0.1.0".to_string(),
                },
                artifact: ArtifactRef {
                    name: "AGENTS".to_string(),
                    surface: Surface::Agent,
                    checksum: "sha256:artifact".to_string(),
                },
                intents: self.intents.clone(),
                dropped: self.dropped.clone(),
            }
        }

        fn catalog(&self) -> Catalog {
            cmf::catalog::scan(Path::new(KB), &self.fs).expect("fixture knowledge base scans")
        }

        fn check(&self, languages: &[&str], runner: &FakeProcessRunner) -> Vec<IntentOutcome> {
            self.check_with(languages, &ProjectConfig::default(), runner)
        }

        fn check_with(
            &self,
            languages: &[&str],
            config: &ProjectConfig,
            runner: &FakeProcessRunner,
        ) -> Vec<IntentOutcome> {
            let manifest = self.manifest();
            let catalog = self.catalog();
            let languages: Vec<String> = languages.iter().map(ToString::to_string).collect();
            let request = CheckRequest {
                manifest: &manifest,
                catalog: &catalog,
                config,
                languages: &languages,
                knowledge_base: Path::new(KB),
                workspace: Path::new(WORKSPACE),
                scratch: Path::new(SCRATCH),
            };
            check(&request, &self.fs, runner).expect("check runs")
        }
    }

    const RUST: &str = "checks/rust/check.py";
    const PYTHON: &str = "checks/python/check.py";

    fn program(run: &str) -> PathBuf {
        Path::new(KB).join(run)
    }

    fn rust_check(required: bool) -> String {
        check_entry("static-check", "rust", RUST, required)
    }

    fn python_check(required: bool) -> String {
        check_entry("static-check", "python", PYTHON, required)
    }

    const PASS: &str = r#"{"applicable": true, "followed": true}"#;
    const FAIL: &str = r#"{"applicable": true, "followed": false, "evidence": ["nope"], "locations": [{"path": "src/a.rs", "line": 3}]}"#;
    const NOT_APPLICABLE: &str = r#"{"applicable": false, "followed": false}"#;

    fn unchecked_reason(outcome: &IntentOutcome) -> &str {
        match &outcome.state {
            State::Unchecked { reason } => reason,
            other => panic!("expected unchecked, got {other:?}"),
        }
    }

    #[test]
    fn passing_validator_yields_pass_with_validator_metadata() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let outcomes = kb.check(&["rust"], &runner);
        assert_eq!(outcomes.len(), 1);
        let outcome = &outcomes[0];
        assert_eq!(outcome.state, State::Pass);
        assert_eq!(outcome.id.as_deref(), Some("kb.intent.a"));
        assert_eq!(outcome.key, "rust/a");
        assert_eq!(outcome.language.as_deref(), Some("rust"));
        assert!(outcome.required);
        assert_eq!(outcome.description.as_deref(), Some("rust check"));
        assert_eq!(outcome.signals, json!({}));
        assert!(!outcome.stale);
    }

    #[test]
    fn validator_is_invoked_per_protocol_with_the_intent_config() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let config: ProjectConfig = toml::from_str(
            "validator_timeout_seconds = 7\n[intent.\"rust/a\"]\nmarkers = [\"x\"]\n",
        )
        .unwrap();
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        kb.check_with(&["rust"], &config, &runner);
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.program, program(RUST));
        assert_eq!(call.cwd, PathBuf::from(KB));
        assert_eq!(call.timeout, Duration::from_secs(7));
        let config_path = PathBuf::from(format!("{SCRATCH}/0.json"));
        assert_eq!(
            call.args,
            vec![
                OsString::from("--workspace"),
                OsString::from(WORKSPACE),
                OsString::from("--config"),
                config_path.clone().into_os_string(),
            ]
        );
        let written: Value =
            serde_json::from_str(&kb.fs.read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(written, json!({ "markers": ["x"] }));
    }

    #[test]
    fn unconfigured_intent_receives_an_empty_object() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        kb.check(&["rust"], &runner);
        let written = kb.fs.read_to_string(Path::new(&format!("{SCRATCH}/0.json"))).unwrap();
        assert_eq!(written, "{}");
    }

    #[test]
    fn failing_validator_carries_evidence_and_locations() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(false)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), FAIL);
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Fail);
        assert!(!outcome.required, "optional validator stays optional");
        assert_eq!(outcome.evidence, ["nope"]);
        assert_eq!(outcome.locations[0].render(), "src/a.rs:3");
    }

    #[test]
    fn not_applicable_verdict_is_reported_as_such() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), NOT_APPLICABLE);
        assert_eq!(kb.check(&["rust"], &runner)[0].state, State::NotApplicable);
    }

    #[test]
    fn record_missing_from_knowledge_base_is_unchecked() {
        let mut kb = Kb::new();
        kb.add_missing("rust/ghost");
        let runner = FakeProcessRunner::new();
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "record not in knowledge base");
        assert_eq!(outcome.id.as_deref(), Some("kb.intent.rust/ghost"));
        assert_eq!(outcome.language, None);
        assert_eq!(outcome.description, None);
        assert!(!outcome.required);
        assert!(!outcome.stale);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn no_validator_for_the_workspace_languages_is_unchecked() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[python_check(true)]);
        let runner = FakeProcessRunner::new();
        let outcome = kb.check(&["go", "rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "no validator for languages [go, rust]");
        assert!(runner.calls().is_empty(), "no validator is started");
        assert!(!kb.fs.exists(Path::new(&format!("{SCRATCH}/0.json"))), "no config is written");
    }

    #[test]
    fn record_without_validators_is_unchecked_with_an_empty_language_list() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[]);
        let outcome = kb.check(&[], &FakeProcessRunner::new()).remove(0);
        assert_eq!(unchecked_reason(&outcome), "no validator for languages []");
    }

    #[test]
    fn timeout_is_unchecked_with_the_configured_seconds() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script(program(RUST), ProcessOutcome::TimedOut);
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "timed out after 60s");
        assert_eq!(outcome.language.as_deref(), Some("rust"), "the validator that ran is named");
        assert!(
            outcome.required,
            "an unchecked required validator stays required for --strict readers"
        );
    }

    #[test]
    fn nonzero_exit_is_unchecked_with_the_first_stderr_line() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script(
            program(RUST),
            exited(3, "partial", "\nTraceback (most recent call last):\n  File x\n"),
        );
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "exited 3: Traceback (most recent call last):");
    }

    #[test]
    fn nonzero_exit_without_stderr_names_only_the_code() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script(program(RUST), exited(1, "", ""));
        assert_eq!(unchecked_reason(&kb.check(&["rust"], &runner)[0]), "exited 1");
    }

    #[test]
    fn signal_death_is_unchecked() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script(
            program(RUST),
            ProcessOutcome::Exited {
                code: None,
                stdout: String::new(),
                stderr: "Segmentation fault\n".to_string(),
            },
        );
        assert_eq!(
            unchecked_reason(&kb.check(&["rust"], &runner)[0]),
            "killed by signal: Segmentation fault"
        );
    }

    #[test]
    fn unparseable_stdout_is_unchecked_with_the_serde_error() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), "not json\n");
        let reason = unchecked_reason(&kb.check(&["rust"], &runner)[0]).to_string();
        assert!(reason.starts_with("invalid verdict JSON: "), "{reason}");
        assert!(reason.contains("line 1"), "{reason}");
    }

    #[test]
    fn missing_executable_is_unchecked_with_the_run_path() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let reason =
            unchecked_reason(&kb.check(&["rust"], &FakeProcessRunner::new())[0]).to_string();
        assert!(reason.starts_with("could not start checks/rust/check.py: "), "{reason}");
    }

    #[test]
    fn changed_record_is_stale_but_still_checked() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.intents[0].checksum = "sha256:stale".to_string();
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert!(outcome.stale);
    }

    #[test]
    fn stale_is_reported_even_when_unchecked_for_lack_of_a_validator() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[]);
        kb.intents[0].checksum = "sha256:stale".to_string();
        let outcome = kb.check(&["rust"], &FakeProcessRunner::new()).remove(0);
        assert!(matches!(outcome.state, State::Unchecked { .. }));
        assert!(outcome.stale);
    }

    #[test]
    fn record_is_resolved_by_id_when_its_key_moved() {
        let mut kb = Kb::new();
        kb.add("rust/new-home", &[rust_check(true)]);
        kb.intents[0].key = "rust/old-home".to_string();
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert_eq!(
            outcome.key, "rust/old-home",
            "the manifest key stays the locator in the report"
        );
    }

    #[test]
    fn record_is_resolved_by_key_when_its_id_changed() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.intents[0].id = "kb.intent.renamed".to_string();
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        assert_eq!(kb.check(&["rust"], &runner)[0].state, State::Pass);
    }

    #[test]
    fn only_validators_for_workspace_languages_run() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true), python_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert_eq!(runner.calls().len(), 1);
        assert_eq!(runner.calls()[0].program, program(RUST));
    }

    #[test]
    fn two_matching_validators_pass_only_when_both_pass() {
        let mut kb = Kb::new();
        kb.add("multi/a", &[rust_check(true), python_check(false)]);
        let both = FakeProcessRunner::new()
            .script_stdout(program(RUST), PASS)
            .script_stdout(program(PYTHON), PASS);
        let outcome = kb.check(&["python", "rust"], &both).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert_eq!(outcome.language.as_deref(), Some("python, rust"));
        assert_eq!(outcome.description.as_deref(), Some("rust check / python check"));
        assert_eq!(outcome.signals, json!({ "rust": {}, "python": {} }));
        assert!(outcome.required, "the required run contributed to the pass");

        let one_fails = FakeProcessRunner::new()
            .script_stdout(program(RUST), PASS)
            .script_stdout(program(PYTHON), FAIL);
        let outcome = kb.check(&["python", "rust"], &one_fails).remove(0);
        assert_eq!(outcome.state, State::Fail);
        assert!(!outcome.required, "only the optional python validator failed");
        assert_eq!(outcome.evidence, ["nope"]);
    }

    #[test]
    fn two_matching_validators_prefer_fail_over_unchecked_over_pass() {
        let mut kb = Kb::new();
        kb.add("multi/a", &[rust_check(true), python_check(true)]);
        let fail_and_timeout = FakeProcessRunner::new()
            .script_stdout(program(RUST), FAIL)
            .script(program(PYTHON), ProcessOutcome::TimedOut);
        assert_eq!(kb.check(&["python", "rust"], &fail_and_timeout)[0].state, State::Fail);

        let pass_and_timeout = FakeProcessRunner::new()
            .script_stdout(program(RUST), PASS)
            .script(program(PYTHON), ProcessOutcome::TimedOut);
        let outcome = kb.check(&["python", "rust"], &pass_and_timeout).remove(0);
        assert_eq!(unchecked_reason(&outcome), "python: timed out after 60s");

        let pass_and_not_applicable = FakeProcessRunner::new()
            .script_stdout(program(RUST), PASS)
            .script_stdout(program(PYTHON), NOT_APPLICABLE);
        assert_eq!(kb.check(&["python", "rust"], &pass_and_not_applicable)[0].state, State::Pass);

        let neither_applicable = FakeProcessRunner::new()
            .script_stdout(program(RUST), NOT_APPLICABLE)
            .script_stdout(program(PYTHON), NOT_APPLICABLE);
        assert_eq!(
            kb.check(&["python", "rust"], &neither_applicable)[0].state,
            State::NotApplicable
        );
    }

    #[test]
    fn dropped_intents_are_unguided_after_the_compiled_ones() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.add("rust/b", &[]);
        kb.drop_key("rust/b", "budget");
        kb.drop_key("rust/never-existed", "budget");
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let outcomes = kb.check(&["rust"], &runner);
        let keys: Vec<_> = outcomes.iter().map(|outcome| outcome.key.as_str()).collect();
        assert_eq!(keys, ["rust/a", "rust/b", "rust/b", "rust/never-existed"]);
        assert_eq!(
            outcomes[2].state,
            State::Unguided {
                reason: "budget".to_string()
            }
        );
        assert_eq!(
            outcomes[2].id.as_deref(),
            Some("kb.intent.b"),
            "id is filled from the record when present"
        );
        assert_eq!(outcomes[3].id, None);
        assert!(!outcomes[3].required);
    }

    #[test]
    fn outcomes_follow_manifest_order_not_catalog_order() {
        let mut kb = Kb::new();
        kb.add("rust/z", &[rust_check(true)]);
        kb.add("rust/a", &[rust_check(true)]);
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let keys: Vec<_> =
            kb.check(&["rust"], &runner).into_iter().map(|outcome| outcome.key).collect();
        assert_eq!(keys, ["rust/z", "rust/a"]);
        let configs: Vec<_> = runner.calls().iter().map(|call| call.args[3].clone()).collect();
        assert_eq!(
            configs,
            [
                OsString::from("/scratch/0.json"),
                OsString::from("/scratch/1.json")
            ]
        );
    }

    #[test]
    fn status_counts_coverage_without_running_anything() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.add("rust/b", &[python_check(true)]);
        kb.add("rust/c", &[]);
        kb.add_missing("rust/ghost");
        kb.drop_key("rust/d", "budget");
        kb.intents[2].checksum = "sha256:stale".to_string();
        let manifest = kb.manifest();
        let catalog = kb.catalog();
        let report = status(
            &manifest,
            Path::new("/project/.context-mixer/cmf-manifest.json"),
            Some(&catalog),
            &["rust".to_string()],
            Path::new(KB),
            &kb.fs,
        )
        .unwrap();
        assert_eq!(
            report,
            StatusReport {
                schema: 1,
                manifest_path: PathBuf::from("/project/.context-mixer/cmf-manifest.json"),
                profile: manifest.profile.clone(),
                artifact: manifest.artifact.clone(),
                knowledge_base: KnowledgeBaseStatus {
                    path: PathBuf::from(KB),
                    exists: true,
                    source: Some("guidelines".to_string()),
                    revision: None,
                },
                languages: vec!["rust".to_string()],
                intents: 4,
                dropped: 1,
                coverage: Some(Coverage {
                    with_validator: 1,
                    missing: 1,
                    stale: 1,
                }),
            }
        );
    }

    #[test]
    fn status_without_a_scannable_knowledge_base_has_no_coverage() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let manifest = kb.manifest();
        let report =
            status(&manifest, Path::new("/m.json"), None, &[], Path::new("/elsewhere"), &kb.fs)
                .unwrap();
        assert!(!report.knowledge_base.exists);
        assert_eq!(report.coverage, None);
        assert_eq!(report.intents, 1);
    }
}
