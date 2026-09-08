//! The verification core: [`check`] resolves every compiled intent against the
//! atlas, runs the validators that match the workspace's ecosystems
//! ([`crate::ecosystems::Ecosystems`]), and maps each run onto exactly one [`IntentOutcome`]; [`status`] answers
//! the same resolution questions without running anything. Pure over the
//! `Filesystem` and [`ProcessRunner`] gateways: the same inputs against the
//! in-memory fakes produce the same outcomes as against the OS.
//!
//! Resolution is by catalog `key` first — the manifest's compile-time locator
//! — then by record `id` only when the key is gone and exactly one record
//! carries the id (a moved record). Ids recur across collections by design
//! (one specialization per language), so an id shared by several records
//! never stands in for a missing key; the intent is reported unchecked with a
//! reason naming those records. An intent is reported
//! `stale` when its record's bytes at the working tree's `HEAD` no longer
//! match the manifest checksum, or — when validators run from a materialized
//! pinned tree (see [`crate::pin`]) — when any of its validators' `run` files
//! differ between `HEAD` and that tree. Stale is always computed against the
//! working tree and never changes the exit code: the remedy is `cmf install`,
//! because a newer atlas may change selection, and that is cmf's
//! decision.
//!
//! When several validators match (a record may declare one per language, and
//! a workspace may have several ecosystems) they combine **all-must-pass**: any
//! failure fails the intent, otherwise any unchecked run leaves it unchecked,
//! otherwise it passes when at least one run was applicable. `CMV.md` lists
//! the alternative (any-passes) as an open decision; all-must-pass is the
//! conservative reading and the one implemented here.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use cmx_core::checksum;
use cmx_core::gateway::Filesystem;
use intent_atlas::catalog::{Intent, Validator};
use intent_atlas::manifest::{ArtifactRef, DroppedIntent, IntentRef, Manifest, ProfileRef};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::ProjectConfig;
use crate::ecosystems::Ecosystems;
use crate::pin::AtlasReport;
use crate::process::{ProcessOutcome, ProcessRequest, ProcessRunner};
use crate::verdict::{self, IntentOutcome, Location, State, empty_object};

/// The scanned atlas, keyed by catalog key.
pub type Catalog = BTreeMap<String, Intent>;

/// The atlas trees a run reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trees<'a> {
    /// The tree the catalog was scanned from and validators run in: the
    /// resolved root, or the materialized pinned tree. Validator `run` paths
    /// resolve against it (canonicalized through the filesystem gateway
    /// before a process is started) and it is the validators' working
    /// directory.
    pub verified: &'a Path,
    /// The atlas's working tree (`HEAD`), only when `verified` is a
    /// materialized pinned tree rather than the working tree itself. Stale is
    /// computed against it.
    pub working: Option<&'a Path>,
}

impl<'a> Trees<'a> {
    /// A run that verifies the working tree itself.
    pub fn single(root: &'a Path) -> Self {
        Self {
            verified: root,
            working: None,
        }
    }
}

/// Everything [`check`] needs besides its gateways.
pub struct CheckRequest<'a> {
    /// The manifest cmf compiled for the workspace.
    pub manifest: &'a Manifest,
    /// The atlas as scanned by `intent_atlas::catalog::scan` from
    /// `trees.verified`.
    pub catalog: &'a Catalog,
    /// The project's `cmv.toml`.
    pub config: &'a ProjectConfig,
    /// The ecosystems to verify as; validators for other languages do not
    /// run.
    pub ecosystems: &'a Ecosystems,
    /// The atlas trees.
    pub trees: Trees<'a>,
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
    let atlas = canonical_root(request.trees.verified, fs)?;
    let mut outcomes =
        Vec::with_capacity(request.manifest.intents.len() + request.manifest.dropped.len());
    for (index, entry) in request.manifest.intents.iter().enumerate() {
        outcomes.push(check_intent(index, entry, request, &atlas, &resolver, fs, runner)?);
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
    atlas: &Path,
    resolver: &Resolver<'_>,
    fs: &dyn Filesystem,
    runner: &dyn ProcessRunner,
) -> Result<IntentOutcome> {
    let intent = match resolver.resolve(entry) {
        Resolution::Found { intent, .. } => intent,
        Resolution::NotFound { detail } => {
            let reason = match detail {
                Some(detail) => format!("{NOT_IN_ATLAS}: {detail}"),
                None => NOT_IN_ATLAS.to_string(),
            };
            return Ok(unchecked(entry, &reason, false));
        }
    };
    let stale = is_stale(intent, entry, request.trees, fs)?;
    let validators: Vec<Validator<'_>> = intent
        .record
        .validators()
        .filter(|validator| request.ecosystems.admits(validator))
        .collect();
    if validators.is_empty() {
        let reason = request.ecosystems.unchecked_reason(&intent.record);
        return Ok(unchecked(entry, &reason, stale));
    }
    let config_path = request.scratch.join(format!("{index}.json"));
    let config_json = serde_json::to_string(&request.config.intent_config(&entry.key))
        .context("could not serialize intent config")?;
    fs.write(&config_path, &config_json)
        .with_context(|| format!("could not write validator config {}", config_path.display()))?;
    let runs: Vec<ValidatorRun> = validators
        .iter()
        .map(|validator| run_validator(validator, atlas, &config_path, request, runner))
        .collect();
    Ok(combine(entry, &runs, stale))
}

/// The absolute form of the tree validators run from. Validators run with it
/// as their working directory, and a relative program path would be looked up
/// against that new directory rather than cmv's; an absolute root makes
/// `<root>/<run>` unambiguous.
pub fn canonical_root(root: &Path, fs: &dyn Filesystem) -> Result<PathBuf> {
    fs.canonicalize(root)
        .with_context(|| format!("could not resolve atlas {}", root.display()))
}

/// Whether the intent has changed at the working tree's `HEAD` since compile:
/// the record's bytes there no longer match the manifest checksum (or the
/// record is gone), or, when validators run from a materialized pinned tree,
/// any validator's `run` file differs between `HEAD` and that tree.
pub fn is_stale(
    intent: &Intent,
    entry: &IntentRef,
    trees: Trees<'_>,
    fs: &dyn Filesystem,
) -> Result<bool> {
    let record_at_head = match trees.working {
        Some(working) => working.join("intents").join(format!("{}.toml", intent.key)),
        None => intent.path.clone(),
    };
    if checksum_if_present(&record_at_head, fs)?.as_deref() != Some(entry.checksum.as_str()) {
        return Ok(true);
    }
    let Some(working) = trees.working else {
        return Ok(false);
    };
    for validator in intent.record.validators() {
        let pinned = checksum_if_present(&trees.verified.join(validator.run), fs)?;
        let head = checksum_if_present(&working.join(validator.run), fs)?;
        if pinned != head {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The file's checksum, or `None` when there is no file there.
fn checksum_if_present(path: &Path, fs: &dyn Filesystem) -> Result<Option<String>> {
    if !fs.is_file(path) {
        return Ok(None);
    }
    checksum::checksum_file(path, fs)
        .map(Some)
        .with_context(|| format!("could not checksum {}", path.display()))
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
    atlas: &Path,
    config_path: &Path,
    request: &CheckRequest<'_>,
    runner: &dyn ProcessRunner,
) -> ValidatorRun {
    let timeout = request.config.validator_timeout();
    let process = ProcessRequest {
        program: atlas.join(validator.run),
        args: validator_args(request.workspace, config_path),
        cwd: atlas.to_path_buf(),
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

/// The arguments every validator receives, per the invocation protocol:
/// `--workspace <project-root> --config <json-file>`. The one definition,
/// shared with `cmv explain` so the argv it prints is the argv `check` uses.
pub fn validator_args(workspace: &Path, config_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("--workspace"),
        workspace.as_os_str().to_owned(),
        OsString::from("--config"),
        config_path.as_os_str().to_owned(),
    ]
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

/// How a manifest entry was matched to an atlas record.
///
/// The manifest's `key` is the compile-time locator and is tried first: a
/// record's `id` may intentionally recur across collections of the atlas
/// (the same intent, specialized per language), so an `id` alone does
/// not locate a record. The manifest checksum, not the resolution, is what
/// flags a changed record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordResolution {
    /// A record sits at the entry's `key`.
    Key,
    /// No record sits at the `key`, but exactly one record carries the
    /// entry's `id`: the record moved since compile.
    Id,
    /// Neither: no record at the `key`, and the `id` matches no record — or
    /// several, none of which can be told apart without the key.
    NotFound,
}

/// The `unchecked` reason for a manifest entry no record matches.
pub const NOT_IN_ATLAS: &str = "record not in atlas";

/// The outcome of matching one manifest entry against the catalog.
#[derive(Debug, Clone)]
pub enum Resolution<'a> {
    /// A record matched, and how.
    Found {
        /// The matched record.
        intent: &'a Intent,
        /// By key, or by a moved record's unique id.
        how: RecordResolution,
    },
    /// Nothing matched. `detail` is set when the fallback by id was stopped
    /// by several records sharing the id; it names them and the remedy.
    NotFound {
        /// Why the id could not stand in for the missing key, if that is
        /// what happened.
        detail: Option<String>,
    },
}

/// Looks records up by `key` first, then by a unique `id`.
pub struct Resolver<'a> {
    catalog: &'a Catalog,
    by_id: BTreeMap<&'a str, Vec<&'a Intent>>,
}

impl<'a> Resolver<'a> {
    /// Index the catalog by record `id`, keeping every record per id in key
    /// order.
    pub fn new(catalog: &'a Catalog) -> Self {
        let mut by_id: BTreeMap<&str, Vec<&Intent>> = BTreeMap::new();
        for intent in catalog.values() {
            by_id.entry(intent.record.id.as_str()).or_default().push(intent);
        }
        Self { catalog, by_id }
    }

    /// The record for a manifest entry, and how it was matched: the record at
    /// the entry's `key`; failing that, the single record carrying its `id`
    /// (a moved record); failing that, not found — with a reason naming the
    /// records that share the id, when several do, so the remedy (`cmf
    /// install` to recompile) is clear.
    pub fn resolve(&self, entry: &IntentRef) -> Resolution<'a> {
        if let Some(intent) = self.by_key(&entry.key) {
            return Resolution::Found {
                intent,
                how: RecordResolution::Key,
            };
        }
        match self.with_id(&entry.id) {
            [intent] => Resolution::Found {
                intent,
                how: RecordResolution::Id,
            },
            [] => Resolution::NotFound { detail: None },
            shared => Resolution::NotFound {
                detail: Some(format!(
                    "no record at key {}, and id {} is shared by {} records ({}); re-run cmf install to recompile",
                    entry.key,
                    entry.id,
                    shared.len(),
                    shared.iter().map(|intent| intent.key.as_str()).collect::<Vec<_>>().join(", ")
                )),
            },
        }
    }

    /// The one record carrying `id`, when exactly one does.
    pub fn by_id(&self, id: &str) -> Option<&'a Intent> {
        match self.with_id(id) {
            [intent] => Some(intent),
            _ => None,
        }
    }

    /// Every record carrying `id`, in key order.
    pub fn with_id(&self, id: &str) -> &[&'a Intent] {
        self.by_id.get(id).map_or(&[], Vec::as_slice)
    }

    /// The record at catalog `key`, if any.
    pub fn by_key(&self, key: &str) -> Option<&'a Intent> {
        self.catalog.get(key)
    }
}

/// What `cmv status` reports: the manifest's identity, the atlas's
/// whereabouts and pin, the workspace's ecosystems, and how much of the
/// manifest the atlas can currently vouch for. Nothing is executed.
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
    /// The atlas as cmv resolved it.
    pub atlas: AtlasStatus,
    /// The ecosystems the workspace verifies as, and where they came from.
    pub ecosystems: Ecosystems,
    /// Compiled intents in the manifest.
    pub intents: usize,
    /// Dropped intents in the manifest.
    pub dropped: usize,
    /// How the atlas covers the manifest; `None` when it could not be
    /// scanned.
    pub coverage: Option<Coverage>,
}

/// Where the atlas is, how cmv found it, how the verified tree
/// relates to the pin, and whether the root is there at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtlasStatus {
    /// The resolution and pin facts every report shares.
    #[serde(flatten)]
    pub resolved: AtlasReport,
    /// Whether the resolved root is a directory.
    pub exists: bool,
}

/// How the scanned atlas covers the manifest's compiled intents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// Compiled intents with at least one validator for the workspace's
    /// ecosystems.
    pub with_validator: usize,
    /// Compiled intents whose record is no longer in the atlas.
    pub missing: usize,
    /// Compiled intents whose record changed since compile.
    pub stale: usize,
}

/// Everything [`status`] needs besides the filesystem.
pub struct StatusRequest<'a> {
    /// The manifest cmf compiled for the workspace.
    pub manifest: &'a Manifest,
    /// Where the manifest was read from.
    pub manifest_path: &'a Path,
    /// The atlas as scanned from `trees.verified`; `None` when it
    /// could not be scanned, in which case the report says where cmv looked
    /// and stops short of coverage.
    pub catalog: Option<&'a Catalog>,
    /// The ecosystems the workspace verifies as.
    pub ecosystems: &'a Ecosystems,
    /// Where the atlas was found and which tree was used.
    pub atlas: &'a AtlasReport,
    /// The atlas trees.
    pub trees: Trees<'a>,
}

/// Summarize the manifest against the atlas without running
/// validators.
pub fn status(request: &StatusRequest<'_>, fs: &dyn Filesystem) -> Result<StatusReport> {
    let coverage = match request.catalog {
        Some(catalog) => {
            Some(coverage(request.manifest, catalog, request.ecosystems, request.trees, fs)?)
        }
        None => None,
    };
    Ok(StatusReport {
        schema: crate::report::SCHEMA_VERSION,
        manifest_path: request.manifest_path.to_path_buf(),
        profile: request.manifest.profile.clone(),
        artifact: request.manifest.artifact.clone(),
        atlas: AtlasStatus {
            exists: fs.is_dir(&request.atlas.path),
            resolved: request.atlas.clone(),
        },
        ecosystems: request.ecosystems.clone(),
        intents: request.manifest.intents.len(),
        dropped: request.manifest.dropped.len(),
        coverage,
    })
}

fn coverage(
    manifest: &Manifest,
    catalog: &Catalog,
    ecosystems: &Ecosystems,
    trees: Trees<'_>,
    fs: &dyn Filesystem,
) -> Result<Coverage> {
    let resolver = Resolver::new(catalog);
    let mut coverage = Coverage {
        with_validator: 0,
        missing: 0,
        stale: 0,
    };
    for entry in &manifest.intents {
        let Resolution::Found { intent, .. } = resolver.resolve(entry) else {
            coverage.missing += 1;
            continue;
        };
        if is_stale(intent, entry, trees, fs)? {
            coverage.stale += 1;
        }
        if intent.record.validators().any(|validator| ecosystems.admits(&validator)) {
            coverage.with_validator += 1;
        }
    }
    Ok(coverage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{FakeProcessRunner, exited};
    use cmx_core::gateway::fakes::FakeFilesystem;
    use intent_atlas::manifest::Atlas;
    use intent_atlas::profile::Surface;
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

    /// An atlas on a fake filesystem plus the manifest entries that
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
                atlas: Atlas {
                    source: Some("guidelines".to_string()),
                    path: PathBuf::from(KB),
                    revision: None,
                },
                profile: ProfileRef {
                    id: "shipping".to_string(),
                    version: "0.1.0".to_string(),
                    ecosystems: vec![],
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
            intent_atlas::catalog::scan(Path::new(KB), &self.fs).expect("fixture atlas scans")
        }

        fn check(&self, ecosystems: &[&str], runner: &FakeProcessRunner) -> Vec<IntentOutcome> {
            self.check_with(ecosystems, &ProjectConfig::default(), runner)
        }

        fn check_with(
            &self,
            ecosystems: &[&str],
            config: &ProjectConfig,
            runner: &FakeProcessRunner,
        ) -> Vec<IntentOutcome> {
            let manifest = self.manifest();
            let catalog = self.catalog();
            let ecosystems = detected(ecosystems);
            let request = CheckRequest {
                manifest: &manifest,
                catalog: &catalog,
                config,
                ecosystems: &ecosystems,
                trees: Trees::single(Path::new(KB)),
                workspace: Path::new(WORKSPACE),
                scratch: Path::new(SCRATCH),
            };
            check(&request, &self.fs, runner).expect("check runs")
        }

        /// Copy the atlas to `PINNED` as the materialized pinned
        /// tree, so `KB` plays the working tree that may since have changed.
        fn materialize_pinned_copy(&self) {
            for (path, bytes) in self.fs.snapshot_files() {
                if let Ok(relative) = path.strip_prefix(KB) {
                    self.fs.add_file(Path::new(PINNED).join(relative), bytes);
                }
            }
            self.fs.add_dir(format!("{PINNED}/intents"));
        }

        /// Check with validators running from the pinned copy and stale
        /// computed against `KB`.
        fn check_pinned(
            &self,
            ecosystems: &[&str],
            runner: &FakeProcessRunner,
        ) -> Vec<IntentOutcome> {
            let manifest = self.manifest();
            let catalog = intent_atlas::catalog::scan(Path::new(PINNED), &self.fs)
                .expect("pinned tree scans");
            let ecosystems = detected(ecosystems);
            let request = CheckRequest {
                manifest: &manifest,
                catalog: &catalog,
                config: &ProjectConfig::default(),
                ecosystems: &ecosystems,
                trees: Trees {
                    verified: Path::new(PINNED),
                    working: Some(Path::new(KB)),
                },
                workspace: Path::new(WORKSPACE),
                scratch: Path::new(SCRATCH),
            };
            check(&request, &self.fs, runner).expect("check runs")
        }
    }

    const PINNED: &str = "/scratch/kb";

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
    fn record_missing_from_atlas_is_unchecked() {
        let mut kb = Kb::new();
        kb.add_missing("rust/ghost");
        let runner = FakeProcessRunner::new();
        let outcome = kb.check(&["rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "record not in atlas");
        assert_eq!(outcome.id.as_deref(), Some("kb.intent.rust/ghost"));
        assert_eq!(outcome.language, None);
        assert_eq!(outcome.description, None);
        assert!(!outcome.required);
        assert!(!outcome.stale);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn no_validator_for_the_workspace_ecosystems_is_unchecked() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[python_check(true)]);
        let runner = FakeProcessRunner::new();
        let outcome = kb.check(&["go", "rust"], &runner).remove(0);
        assert_eq!(unchecked_reason(&outcome), "no validator for ecosystems [go, rust]");
        assert!(runner.calls().is_empty(), "no validator is started");
        assert!(!kb.fs.exists(Path::new(&format!("{SCRATCH}/0.json"))), "no config is written");
    }

    #[test]
    fn record_without_validators_is_unchecked_with_an_empty_ecosystem_list() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[]);
        let outcome = kb.check(&[], &FakeProcessRunner::new()).remove(0);
        assert_eq!(unchecked_reason(&outcome), "no validator for ecosystems []");
    }

    #[test]
    fn without_sensors_every_validator_bearing_intent_is_unchecked_with_the_override_hint() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.add("rust/b", &[]);
        let manifest = kb.manifest();
        let catalog = kb.catalog();
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        let request = CheckRequest {
            manifest: &manifest,
            catalog: &catalog,
            config: &ProjectConfig::default(),
            ecosystems: &Ecosystems::Undetectable,
            trees: Trees::single(Path::new(KB)),
            workspace: Path::new(WORKSPACE),
            scratch: Path::new(SCRATCH),
        };
        let outcomes = check(&request, &kb.fs, &runner).unwrap();
        assert_eq!(
            unchecked_reason(&outcomes[0]),
            "atlas declares no sensors; set ecosystems in cmv.toml to override"
        );
        assert_eq!(unchecked_reason(&outcomes[1]), "no validator for ecosystems []");
        assert!(runner.calls().is_empty(), "nothing runs without a detected ecosystem");
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
    fn pinned_tree_runs_the_pinned_validator_and_is_not_stale_when_head_is_unchanged() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.fs.add_file(format!("{KB}/{RUST}"), "pinned script");
        kb.materialize_pinned_copy();
        let runner = FakeProcessRunner::new().script_stdout(Path::new(PINNED).join(RUST), PASS);
        let outcome = kb.check_pinned(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert!(!outcome.stale);
        let call = &runner.calls()[0];
        assert_eq!(call.program, Path::new(PINNED).join(RUST), "the pinned script runs");
        assert_eq!(call.cwd, PathBuf::from(PINNED), "from the pinned tree");
    }

    #[test]
    fn record_changed_at_head_is_stale_even_though_the_pinned_record_matches() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.materialize_pinned_copy();
        kb.fs.add_file(format!("{KB}/intents/rust/a.toml"), "edited at HEAD");
        let runner = FakeProcessRunner::new().script_stdout(Path::new(PINNED).join(RUST), PASS);
        let outcome = kb.check_pinned(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass, "the pinned record still resolves and runs");
        assert!(outcome.stale, "stale compares HEAD, not the pinned tree");
    }

    #[test]
    fn record_removed_at_head_is_stale() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.materialize_pinned_copy();
        kb.fs.remove_file(Path::new(&format!("{KB}/intents/rust/a.toml"))).unwrap();
        let runner = FakeProcessRunner::new().script_stdout(Path::new(PINNED).join(RUST), PASS);
        assert!(kb.check_pinned(&["rust"], &runner)[0].stale);
    }

    #[test]
    fn validator_changed_between_head_and_pinned_tree_is_stale() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.fs.add_file(format!("{KB}/{RUST}"), "pinned script");
        kb.materialize_pinned_copy();
        kb.fs.add_file(format!("{KB}/{RUST}"), "corrected script at HEAD");
        let runner = FakeProcessRunner::new().script_stdout(Path::new(PINNED).join(RUST), PASS);
        let outcome = kb.check_pinned(&["rust"], &runner).remove(0);
        assert_eq!(outcome.state, State::Pass);
        assert!(outcome.stale, "a corrected validator makes the intent stale");
    }

    #[test]
    fn validator_added_at_head_only_is_stale_and_absent_on_both_sides_is_not() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.materialize_pinned_copy();
        let runner = FakeProcessRunner::new().script_stdout(Path::new(PINNED).join(RUST), PASS);
        assert!(!kb.check_pinned(&["rust"], &runner)[0].stale, "missing on both sides is equal");
        kb.fs.add_file(format!("{KB}/{RUST}"), "new at HEAD");
        assert!(kb.check_pinned(&["rust"], &runner)[0].stale);
    }

    #[test]
    fn without_a_working_tree_only_the_record_check_applies() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.fs.add_file(format!("{KB}/{RUST}"), "script");
        let runner = FakeProcessRunner::new().script_stdout(program(RUST), PASS);
        assert!(!kb.check(&["rust"], &runner)[0].stale);
    }

    #[test]
    fn resolver_prefers_the_key_even_when_the_id_is_shared() {
        let mut kb = Kb::new();
        // Both records carry `kb.intent.a`; the manifest key picks the second.
        kb.add("python/a", &[]).add("rust/a", &[]);
        let catalog = kb.catalog();
        let resolver = Resolver::new(&catalog);
        let Resolution::Found { intent, how } = resolver.resolve(&kb.intents[1]) else {
            panic!("keyed record resolves");
        };
        assert_eq!(intent.key, "rust/a");
        assert_eq!(how, RecordResolution::Key);
        assert_eq!(resolver.with_id("kb.intent.a").len(), 2);
        assert!(resolver.by_id("kb.intent.a").is_none(), "a shared id names no single record");
    }

    #[test]
    fn resolver_falls_back_to_a_unique_id_when_the_key_moved() {
        let mut kb = Kb::new();
        kb.add("rust/new-home", &[]);
        let catalog = kb.catalog();
        let resolver = Resolver::new(&catalog);
        let mut moved = kb.intents[0].clone();
        moved.key = "rust/old-home".to_string();
        let Resolution::Found { intent, how } = resolver.resolve(&moved) else {
            panic!("moved record resolves by its unique id");
        };
        assert_eq!(intent.key, "rust/new-home");
        assert_eq!(how, RecordResolution::Id);
    }

    #[test]
    fn resolver_refuses_to_guess_among_records_sharing_the_id() {
        let mut kb = Kb::new();
        kb.add("python/a", &[]).add("rust/a", &[]);
        let catalog = kb.catalog();
        let resolver = Resolver::new(&catalog);
        let mut moved = kb.intents[1].clone();
        moved.key = "rust/gone".to_string();
        let Resolution::NotFound { detail } = resolver.resolve(&moved) else {
            panic!("a shared id does not locate a record");
        };
        assert_eq!(
            detail.as_deref(),
            Some(
                "no record at key rust/gone, and id kb.intent.a is shared by 2 records (python/a, rust/a); re-run cmf install to recompile"
            )
        );
    }

    #[test]
    fn check_reports_a_shared_id_miss_as_unchecked_with_the_records_named() {
        let mut kb = Kb::new();
        kb.add("python/a", &[]).add("rust/a", &[rust_check(true)]);
        kb.intents[1].key = "rust/gone".to_string();
        let outcome = kb.check(&["rust"], &FakeProcessRunner::new()).remove(1);
        assert_eq!(
            unchecked_reason(&outcome),
            "record not in atlas: no record at key rust/gone, and id kb.intent.a is shared by 2 records (python/a, rust/a); re-run cmf install to recompile"
        );
    }

    #[test]
    fn resolver_reports_not_found_when_neither_key_nor_id_match() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[]);
        let catalog = kb.catalog();
        let resolver = Resolver::new(&catalog);
        let mut gone = kb.intents[0].clone();
        gone.key = "rust/gone".to_string();
        gone.id = "kb.intent.gone".to_string();
        let Resolution::NotFound { detail } = resolver.resolve(&gone) else {
            panic!("nothing matches");
        };
        assert_eq!(detail, None);
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
    fn only_validators_for_workspace_ecosystems_run() {
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
        let atlas = atlas_report(KB);
        let request = StatusRequest {
            manifest: &manifest,
            manifest_path: Path::new("/project/.context-mixer/cmf-manifest.json"),
            catalog: Some(&catalog),
            ecosystems: &detected(&["rust"]),
            atlas: &atlas,
            trees: Trees::single(Path::new(KB)),
        };
        let report = status(&request, &kb.fs).unwrap();
        assert_eq!(
            report,
            StatusReport {
                schema: 1,
                manifest_path: PathBuf::from("/project/.context-mixer/cmf-manifest.json"),
                profile: manifest.profile.clone(),
                artifact: manifest.artifact.clone(),
                atlas: AtlasStatus {
                    resolved: atlas,
                    exists: true,
                },
                ecosystems: detected(&["rust"]),
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
    fn status_counts_validator_drift_against_the_pinned_tree_as_stale() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        kb.fs.add_file(format!("{KB}/{RUST}"), "pinned script");
        kb.materialize_pinned_copy();
        kb.fs.add_file(format!("{KB}/{RUST}"), "corrected at HEAD");
        let manifest = kb.manifest();
        let catalog = intent_atlas::catalog::scan(Path::new(PINNED), &kb.fs).unwrap();
        let atlas = atlas_report(KB);
        let request = StatusRequest {
            manifest: &manifest,
            manifest_path: Path::new("/m.json"),
            catalog: Some(&catalog),
            ecosystems: &detected(&["rust"]),
            atlas: &atlas,
            trees: Trees {
                verified: Path::new(PINNED),
                working: Some(Path::new(KB)),
            },
        };
        let report = status(&request, &kb.fs).unwrap();
        assert_eq!(report.coverage.unwrap().stale, 1);
    }

    #[test]
    fn status_without_a_scannable_atlas_has_no_coverage() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let manifest = kb.manifest();
        let atlas = atlas_report("/elsewhere");
        let request = StatusRequest {
            manifest: &manifest,
            manifest_path: Path::new("/m.json"),
            catalog: None,
            ecosystems: &Ecosystems::Undetectable,
            atlas: &atlas,
            trees: Trees::single(Path::new("/elsewhere")),
        };
        let report = status(&request, &kb.fs).unwrap();
        assert!(!report.atlas.exists);
        assert_eq!(report.coverage, None);
        assert_eq!(report.intents, 1);
    }

    #[test]
    fn status_without_sensors_counts_no_validator_as_selectable() {
        let mut kb = Kb::new();
        kb.add("rust/a", &[rust_check(true)]);
        let manifest = kb.manifest();
        let catalog = kb.catalog();
        let atlas = atlas_report(KB);
        let request = StatusRequest {
            manifest: &manifest,
            manifest_path: Path::new("/m.json"),
            catalog: Some(&catalog),
            ecosystems: &Ecosystems::Undetectable,
            atlas: &atlas,
            trees: Trees::single(Path::new(KB)),
        };
        let report = status(&request, &kb.fs).unwrap();
        assert_eq!(report.coverage.unwrap().with_validator, 0);
        assert_eq!(report.ecosystems, Ecosystems::Undetectable);
    }

    fn detected(names: &[&str]) -> Ecosystems {
        Ecosystems::Detected(names.iter().map(ToString::to_string).collect())
    }

    fn atlas_report(path: &str) -> AtlasReport {
        AtlasReport {
            path: PathBuf::from(path),
            resolved_by: crate::resolve::ResolvedBy::Path,
            source: Some("guidelines".to_string()),
            pinned_revision: None,
            head_revision: None,
            verified_against: crate::pin::VerifiedAgainst::Head,
            moved: false,
            sensors: false,
        }
    }
}
