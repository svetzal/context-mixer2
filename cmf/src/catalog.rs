//! Read-only discovery, parsing, and schema validation of TOML intent records.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use cmx_core::gateway::Filesystem;
use serde::Deserialize;

/// Evidence `type` whose entry declares an executable validator.
///
/// A `static-check` entry must carry both `language` and `run`; no other
/// evidence type may carry either. cmf only records and renders these entries
/// — executing them is the verifier's job (see `CMV.md`).
pub const STATIC_CHECK: &str = "static-check";

/// Directed relationship between intent records.
#[derive(Debug, Clone, Deserialize)]
pub struct Relation {
    /// Relationship type, such as `specializes` or `related-to`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Repository-relative intent key.
    pub target: String,
}

/// Observable evidence associated with an intent.
#[derive(Debug, Clone, Deserialize)]
pub struct Evidence {
    /// Evidence type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Human-readable evidence expectation; the text an agent sees.
    pub description: String,
    /// Whether the evidence is mandatory.
    #[serde(default)]
    pub required: bool,
    /// Source language a validator reads. Present only on [`STATIC_CHECK`]
    /// entries.
    #[serde(default)]
    pub language: Option<String>,
    /// Validator executable, relative to the knowledge-base root. Present only
    /// on [`STATIC_CHECK`] entries.
    #[serde(default)]
    pub run: Option<String>,
}

impl Evidence {
    /// The executable validator this entry declares, if it is a
    /// [`STATIC_CHECK`] entry.
    ///
    /// Entries returned by [`scan`] are already validated, so a
    /// `static-check` entry always yields `Some`; the accessor stays total
    /// rather than panicking on a hand-built half-declared entry.
    pub fn validator(&self) -> Option<Validator<'_>> {
        if self.kind != STATIC_CHECK {
            return None;
        }
        Some(Validator {
            language: self.language.as_deref()?,
            run: Path::new(self.run.as_deref()?),
            required: self.required,
            description: &self.description,
        })
    }
}

/// An executable validator declared by a [`STATIC_CHECK`] evidence entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validator<'a> {
    /// Source language the validator reads, such as `rust` or `python`.
    pub language: &'a str,
    /// Executable path relative to the knowledge-base root.
    pub run: &'a Path,
    /// Whether a failing verdict gates the verification run.
    pub required: bool,
    /// What the validator checks, as rendered into the guidance.
    pub description: &'a str,
}

/// Intent fields consumed during selection and assembly.
#[derive(Debug, Clone, Deserialize)]
pub struct IntentRecord {
    /// Stable semantic identifier.
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// Primary category.
    pub category: String,
    /// Topical tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Maturity state.
    pub status: String,
    /// Intended outcome.
    pub capability: String,
    /// Failure mode addressed.
    pub threat: String,
    /// Testable belief behind the strategy.
    pub expectation: String,
    /// Actionable guidance.
    pub strategy: String,
    /// Cost or downside.
    pub tradeoff: String,
    /// Semantic graph edges.
    #[serde(default)]
    pub relations: Vec<Relation>,
    /// Completion or quality evidence.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

impl IntentRecord {
    /// Every executable validator this record declares, in declaration order.
    pub fn validators(&self) -> impl Iterator<Item = Validator<'_>> {
        self.evidence.iter().filter_map(Evidence::validator)
    }
}

/// Parsed intent plus its knowledge-base identity.
#[derive(Debug, Clone)]
pub struct Intent {
    /// Path-derived key below `intents/`, without `.toml`.
    pub key: String,
    /// Full source path.
    pub path: PathBuf,
    /// Parsed record.
    pub record: IntentRecord,
}

/// Read all structured intent records below `<root>/intents/`.
///
/// Fails on the first record whose evidence entries break the validator
/// schema (see [`STATIC_CHECK`]), naming the record and the entry.
pub fn scan(root: &Path, fs: &dyn Filesystem) -> Result<BTreeMap<String, Intent>> {
    let base = root.join("intents");
    if !fs.is_dir(&base) {
        bail!("{} has no intents/ directory", root.display());
    }
    let mut paths = Vec::new();
    walk(&base, fs, &mut paths)?;
    paths.sort();
    let mut intents = BTreeMap::new();
    for path in paths {
        let raw = fs.read_to_string(&path)?;
        let record: IntentRecord = toml::from_str(&raw)
            .with_context(|| format!("could not parse intent {}", path.display()))?;
        validate_evidence(&path, &record)?;
        let relative = path.strip_prefix(&base)?;
        let mut key = relative.to_path_buf();
        key.set_extension("");
        let key = key.to_string_lossy().replace('\\', "/");
        if intents
            .insert(
                key.clone(),
                Intent {
                    key: key.clone(),
                    path,
                    record,
                },
            )
            .is_some()
        {
            bail!("duplicate intent key {key}");
        }
    }
    Ok(intents)
}

/// Count profile TOML files below `<root>/profiles/`.
pub fn profile_count(root: &Path, fs: &dyn Filesystem) -> Result<usize> {
    let base = root.join("profiles");
    if !fs.is_dir(&base) {
        return Ok(0);
    }
    let mut paths = Vec::new();
    walk(&base, fs, &mut paths)?;
    Ok(paths.len())
}

fn walk(directory: &Path, fs: &dyn Filesystem, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs.read_dir(directory)? {
        if entry.is_dir {
            walk(&entry.path, fs, paths)?;
        } else if entry.path.extension().is_some_and(|extension| extension == "toml") {
            paths.push(entry.path);
        }
    }
    Ok(())
}

fn validate_evidence(path: &Path, record: &IntentRecord) -> Result<()> {
    for (index, entry) in record.evidence.iter().enumerate() {
        if let Some(reason) = evidence_violation(entry) {
            bail!(
                "intent {} evidence entry {} (type {:?}): {reason}",
                path.display(),
                index + 1,
                entry.kind
            );
        }
    }
    Ok(())
}

/// Why `entry` breaks the validator schema, or `None` when it is well-formed.
fn evidence_violation(entry: &Evidence) -> Option<String> {
    if entry.kind == STATIC_CHECK {
        static_check_violation(entry)
    } else {
        non_executable_violation(entry)
    }
}

fn static_check_violation(entry: &Evidence) -> Option<String> {
    let missing = validator_fields(entry, |field| field.is_none_or(str::is_empty));
    if !missing.is_empty() {
        return Some(format!("static-check evidence must declare {}", missing.join(" and ")));
    }
    entry.run.as_deref().and_then(run_path_violation)
}

fn non_executable_violation(entry: &Evidence) -> Option<String> {
    let present = validator_fields(entry, |field| field.is_some());
    if present.is_empty() {
        return None;
    }
    Some(format!(
        "{} may only appear on {STATIC_CHECK:?} evidence",
        present.join(" and ")
    ))
}

/// Names of the validator-only fields (`language`, `run`) for which
/// `matches` holds, ready to be listed in a message.
fn validator_fields(entry: &Evidence, matches: impl Fn(Option<&str>) -> bool) -> Vec<&'static str> {
    [
        ("`language`", entry.language.as_deref()),
        ("`run`", entry.run.as_deref()),
    ]
    .into_iter()
    .filter(|(_, value)| matches(*value))
    .map(|(name, _)| name)
    .collect()
}

/// A validator path resolves against the knowledge-base root and must stay
/// inside it: relative, and never climbing through `..`.
fn run_path_violation(run: &str) -> Option<String> {
    for component in Path::new(run).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Some(format!("validator path {run:?} must not contain `..`"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Some(format!(
                    "validator path {run:?} must be relative to the knowledge-base root"
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;

    const ROOT: &str = "/kb";
    const RECORD_PATH: &str =
        "/kb/intents/craftsperson/rust/put-gateways-at-effect-boundaries.toml";

    const BASE_RECORD: &str = r#"
id = "kb.intent.put-gateways-at-effect-boundaries"
title = "Put gateways at effect boundaries"
category = "architecture"
tags = ["rust"]
status = "confirmed"
capability = "Core logic runs against in-memory fakes."
threat = "Vendor clients leak into business rules."
expectation = "Every effect crosses a project-owned trait."
strategy = "Wrap effects behind gateway traits."
tradeoff = "One more type per effect."
"#;

    const REVIEW_ENTRY: &str = r#"{ type = "architecture_review", description = "Core modules depend on gateway contracts.", required = true }"#;
    const RUST_CHECK_ENTRY: &str = r#"{ type = "static-check", language = "rust", run = "checks/rust/put_gateways_at_effect_boundaries.py", description = "No effectful call leaves a gateway module.", required = true }"#;
    const PYTHON_CHECK_ENTRY: &str = r#"{ type = "static-check", language = "python", run = "checks/python/put_gateways_at_effect_boundaries.py", description = "No effectful call leaves a gateway module.", required = false }"#;

    fn scan_single(evidence: &[&str]) -> Result<BTreeMap<String, Intent>> {
        let fs = FakeFilesystem::new();
        let raw = format!("{BASE_RECORD}evidence = [\n  {},\n]\n", evidence.join(",\n  "));
        fs.add_file(RECORD_PATH, raw);
        scan(Path::new(ROOT), &fs)
    }

    fn single_record(evidence: &[&str]) -> IntentRecord {
        let intents = scan_single(evidence).expect("record scans");
        intents.into_values().next().expect("one record").record
    }

    fn rejection(evidence: &[&str]) -> String {
        let error = scan_single(evidence).expect_err("record is rejected");
        format!("{error:#}")
    }

    #[test]
    fn parses_static_check_evidence_as_a_validator() {
        let record = single_record(&[RUST_CHECK_ENTRY]);
        let validator = record.evidence[0].validator().expect("static-check declares a validator");
        assert_eq!(
            validator,
            Validator {
                language: "rust",
                run: Path::new("checks/rust/put_gateways_at_effect_boundaries.py"),
                required: true,
                description: "No effectful call leaves a gateway module.",
            }
        );
    }

    #[test]
    fn architecture_review_is_not_a_validator() {
        let record = single_record(&[REVIEW_ENTRY]);
        assert_eq!(record.evidence[0].kind, "architecture_review");
        assert!(record.evidence[0].validator().is_none());
        assert_eq!(record.validators().count(), 0);
    }

    #[test]
    fn validators_lists_every_declared_language_in_order() {
        let record = single_record(&[REVIEW_ENTRY, RUST_CHECK_ENTRY, PYTHON_CHECK_ENTRY]);
        let languages: Vec<_> = record.validators().map(|validator| validator.language).collect();
        assert_eq!(languages, ["rust", "python"]);
        let required: Vec<_> = record.validators().map(|validator| validator.required).collect();
        assert_eq!(required, [true, false]);
    }

    #[test]
    fn records_without_validator_fields_still_parse() {
        let fs = FakeFilesystem::new();
        fs.add_file(RECORD_PATH, BASE_RECORD);
        let intents = scan(Path::new(ROOT), &fs).expect("legacy record scans");
        let record = &intents["craftsperson/rust/put-gateways-at-effect-boundaries"].record;
        assert!(record.evidence.is_empty());
        assert_eq!(record.validators().count(), 0);
    }

    #[test]
    fn rejects_static_check_without_run() {
        let message = rejection(&[
            r#"{ type = "static-check", language = "rust", description = "Checked.", required = true }"#,
        ]);
        assert_eq!(
            message,
            format!(
                "intent {RECORD_PATH} evidence entry 1 (type \"static-check\"): static-check evidence must declare `run`"
            )
        );
    }

    #[test]
    fn rejects_static_check_without_language() {
        let message = rejection(&[
            REVIEW_ENTRY,
            r#"{ type = "static-check", run = "checks/rust/x.py", description = "Checked.", required = true }"#,
        ]);
        assert_eq!(
            message,
            format!(
                "intent {RECORD_PATH} evidence entry 2 (type \"static-check\"): static-check evidence must declare `language`"
            )
        );
    }

    #[test]
    fn rejects_static_check_missing_both_validator_fields() {
        let message = rejection(&[r#"{ type = "static-check", description = "Checked." }"#]);
        assert!(
            message.ends_with("static-check evidence must declare `language` and `run`"),
            "{message}"
        );
    }

    #[test]
    fn rejects_validator_fields_on_non_executable_evidence() {
        let message = rejection(&[
            r#"{ type = "architecture_review", run = "checks/rust/x.py", description = "Reviewed.", required = true }"#,
        ]);
        assert_eq!(
            message,
            format!(
                "intent {RECORD_PATH} evidence entry 1 (type \"architecture_review\"): `run` may only appear on \"static-check\" evidence"
            )
        );

        let message = rejection(&[
            r#"{ type = "architecture_review", language = "rust", run = "checks/rust/x.py", description = "Reviewed." }"#,
        ]);
        assert!(
            message.ends_with("`language` and `run` may only appear on \"static-check\" evidence"),
            "{message}"
        );
    }

    #[test]
    fn rejects_absolute_validator_path() {
        let message = rejection(&[
            r#"{ type = "static-check", language = "rust", run = "/usr/bin/check", description = "Checked.", required = true }"#,
        ]);
        assert_eq!(
            message,
            format!(
                "intent {RECORD_PATH} evidence entry 1 (type \"static-check\"): validator path \"/usr/bin/check\" must be relative to the knowledge-base root"
            )
        );
    }

    #[test]
    fn rejects_validator_path_escaping_the_knowledge_base() {
        let message = rejection(&[
            r#"{ type = "static-check", language = "rust", run = "checks/../../outside.py", description = "Checked.", required = true }"#,
        ]);
        assert_eq!(
            message,
            format!(
                "intent {RECORD_PATH} evidence entry 1 (type \"static-check\"): validator path \"checks/../../outside.py\" must not contain `..`"
            )
        );
    }

    #[test]
    fn accepts_validator_path_with_current_directory_component() {
        let record = single_record(&[
            r#"{ type = "static-check", language = "rust", run = "./checks/rust/x.py", description = "Checked.", required = true }"#,
        ]);
        assert_eq!(record.validators().count(), 1);
    }
}
