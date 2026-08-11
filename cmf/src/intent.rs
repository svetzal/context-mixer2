//! Intent-record discovery, parsing, and graph validation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cmx::gateway::Filesystem;
use serde::Deserialize;

use crate::repo::RepoRoot;
use crate::validation::ValidationIssue;

/// A typed relationship from one intent record to another.
#[derive(Debug, Clone, Deserialize)]
pub struct IntentRelation {
    /// Relationship semantics, currently `specializes` or `related-to`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Repository-relative record key without the `.toml` suffix.
    pub target: String,
}

/// Evidence by which an intent's effect can be checked.
#[derive(Debug, Clone, Deserialize)]
pub struct IntentEvidence {
    /// Evidence class, such as `gate` or `completion_report`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Observable evidence expected from the work.
    pub description: String,
    /// Whether materialization may omit this evidence.
    #[serde(default)]
    pub required: bool,
}

/// Provenance supporting an intent record.
#[derive(Debug, Clone, Deserialize)]
pub struct IntentSource {
    /// Provenance class, such as `document`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Stable source reference.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Concise explanation of what the source supports.
    pub summary: String,
    /// Confidence assigned to this source.
    pub confidence: f64,
}

/// The authoring fields cmf needs to index and validate an intent record.
///
/// Additional TOML tables such as `examples`, `scope`, `sources`, and future
/// harness metadata remain valid input; serde intentionally ignores fields
/// that are not needed for indexing.
#[derive(Debug, Clone, Deserialize)]
pub struct IntentRecord {
    /// Globally meaningful semantic identifier.
    pub id: String,
    /// Human-readable statement of the intent.
    pub title: String,
    /// Stable primary area of concern.
    pub category: String,
    /// Lightweight topical classification.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Maturity state of the guidance.
    pub status: String,
    /// Confidence in the record's current formulation.
    pub confidence: f64,
    /// Beneficial outcome enabled by the guidance.
    pub capability: String,
    /// Failure mode the guidance addresses.
    pub threat: String,
    /// Falsifiable expectation behind the strategy.
    pub expectation: String,
    /// Actionable guidance delivered to an agent.
    pub strategy: String,
    /// Cost or downside of following the strategy.
    pub tradeoff: String,
    /// Directed semantic graph relationships.
    #[serde(default)]
    pub relations: Vec<IntentRelation>,
    /// Observable checks supporting the intent.
    pub evidence: Vec<IntentEvidence>,
    /// Repository scope from which the intent was derived.
    pub scope: toml::Table,
    /// Source material supporting the record.
    pub sources: Vec<IntentSource>,
}

/// An intent record plus its repository identity and source path.
#[derive(Debug, Clone)]
pub struct Intent {
    /// Repository-relative key, derived from the path below `intents/`.
    pub key: String,
    /// Parsed source record.
    pub record: IntentRecord,
    /// Full path to the TOML source.
    pub path: PathBuf,
}

/// Wrapper used by the human-readable intent list.
pub struct IntentList(pub Vec<Intent>);

/// Recursively discover all TOML intent records below `intents/`.
pub fn scan_intent_paths(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<PathBuf>> {
    if !root.has_intents {
        return Ok(Vec::new());
    }

    let intents_dir = root.path.join("intents");
    let mut paths = Vec::new();
    walk_intent_paths(&intents_dir, fs, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn walk_intent_paths(
    directory: &Path,
    fs: &dyn Filesystem,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs.read_dir(directory)? {
        if entry.is_dir {
            walk_intent_paths(&entry.path, fs, paths)?;
        } else if entry.path.extension().is_some_and(|extension| extension == "toml") {
            paths.push(entry.path);
        }
    }
    Ok(())
}

/// Parse every discovered intent record, failing when any source is malformed.
pub fn scan_intents(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<Intent>> {
    scan_intent_paths(root, fs)?
        .into_iter()
        .map(|path| load_intent(root, &path, fs))
        .collect()
}

fn load_intent(root: &RepoRoot, path: &Path, fs: &dyn Filesystem) -> Result<Intent> {
    let raw = fs.read_to_string(path)?;
    let record: IntentRecord = toml::from_str(&raw)
        .with_context(|| format!("could not parse intent {}", path.display()))?;
    Ok(Intent {
        key: intent_key(root, path)?,
        record,
        path: path.to_path_buf(),
    })
}

fn intent_key(root: &RepoRoot, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root.path.join("intents"))
        .with_context(|| format!("intent is outside intents/: {}", path.display()))?;
    let mut key = relative.to_path_buf();
    key.set_extension("");
    Ok(key.to_string_lossy().replace('\\', "/"))
}

/// Validate every intent record and all of its graph edges.
pub fn validate_intents(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let paths = scan_intent_paths(root, fs)?;
    let mut intents = Vec::new();
    let mut issues = Vec::new();

    for path in paths {
        match load_intent(root, &path, fs) {
            Ok(intent) => intents.push(intent),
            Err(error) => {
                issues.push(ValidationIssue::error(path.display().to_string(), error.to_string()));
            }
        }
    }

    let keys: BTreeSet<&str> = intents.iter().map(|intent| intent.key.as_str()).collect();
    for intent in &intents {
        validate_record(intent, &keys, &mut issues);
    }

    Ok(issues)
}

fn validate_record(intent: &Intent, keys: &BTreeSet<&str>, issues: &mut Vec<ValidationIssue>) {
    let record = &intent.record;
    let required = [
        ("id", record.id.as_str()),
        ("title", record.title.as_str()),
        ("category", record.category.as_str()),
        ("status", record.status.as_str()),
        ("capability", record.capability.as_str()),
        ("threat", record.threat.as_str()),
        ("expectation", record.expectation.as_str()),
        ("strategy", record.strategy.as_str()),
        ("tradeoff", record.tradeoff.as_str()),
    ];
    for (field, value) in required {
        if value.trim().is_empty() {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                format!("{field} must not be empty"),
            ));
        }
    }

    if !(0.0..=1.0).contains(&record.confidence) {
        issues
            .push(ValidationIssue::error(intent.key.clone(), "confidence must be between 0 and 1"));
    }

    if record.tags.is_empty() {
        issues.push(ValidationIssue::error(intent.key.clone(), "tags must not be empty"));
    }
    if record.evidence.is_empty() {
        issues.push(ValidationIssue::error(intent.key.clone(), "evidence must not be empty"));
    }
    if record.sources.is_empty() {
        issues.push(ValidationIssue::error(intent.key.clone(), "sources must not be empty"));
    }
    if record.scope.is_empty() {
        issues.push(ValidationIssue::error(intent.key.clone(), "scope must not be empty"));
    }
    for evidence in &record.evidence {
        if evidence.kind.trim().is_empty() || evidence.description.trim().is_empty() {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                "evidence type and description must not be empty",
            ));
        }
    }
    for source in &record.sources {
        if source.kind.trim().is_empty()
            || source.reference.trim().is_empty()
            || source.summary.trim().is_empty()
        {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                "source type, ref, and summary must not be empty",
            ));
        }
        if !(0.0..=1.0).contains(&source.confidence) {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                "source confidence must be between 0 and 1",
            ));
        }
    }

    let expected_suffix = intent.key.rsplit('/').next().unwrap_or(&intent.key);
    if intent.path.file_stem().and_then(|stem| stem.to_str()) != Some(expected_suffix) {
        issues.push(ValidationIssue::error(
            intent.key.clone(),
            "intent key does not match its filename",
        ));
    }

    for relation in &record.relations {
        if !matches!(relation.kind.as_str(), "specializes" | "related-to") {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                format!("unknown relationship type {:?}", relation.kind),
            ));
        }
        if !keys.contains(relation.target.as_str()) {
            issues.push(ValidationIssue::error(
                intent.key.clone(),
                format!("relationship target {:?} does not exist", relation.target),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cmx::gateway::fakes::FakeFilesystem;

    use super::*;
    use crate::repo::{RepoKind, RepoRoot};

    fn intent_root(fs: &FakeFilesystem) -> RepoRoot {
        fs.add_dir("/repo/intents");
        RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::IntentsOnly,
            has_intents: true,
            has_plugins_dir: false,
        }
    }

    fn record(id: &str, relations: &str) -> String {
        format!(
            r#"id = "{id}"
title = "Do the thing"
category = "quality"
tags = ["testing"]
status = "hypothesized"
confidence = 0.9
capability = "Reliable delivery"
threat = "Unchecked work"
expectation = "Checks expose defects"
strategy = "Run the checks"
tradeoff = "Checks take time"
evidence = [{{ type = "gate", description = "Checks pass", required = true }}]
scope = {{ project = "guidelines", paths = ["agents/test.md"] }}
sources = [{{ type = "document", ref = "agents/test.md", summary = "Requires checks", confidence = 1.0 }}]
{relations}
"#
        )
    }

    #[test]
    fn scans_nested_intents_and_derives_keys() {
        let fs = FakeFilesystem::new();
        let root = intent_root(&fs);
        fs.add_file(
            "/repo/intents/craftsperson/rust/test.toml",
            record("guidelines.intent.test", ""),
        );

        let intents = scan_intents(&root, &fs).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].key, "craftsperson/rust/test");
        assert_eq!(intents[0].record.category, "quality");
    }

    #[test]
    fn validation_reports_dangling_relationship() {
        let fs = FakeFilesystem::new();
        let root = intent_root(&fs);
        fs.add_file(
            "/repo/intents/craftsperson/test.toml",
            record(
                "guidelines.intent.test",
                "relations = [{ type = \"specializes\", target = \"craftsperson/missing\" }]",
            ),
        );

        let issues = validate_intents(&root, &fs).unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("does not exist"));
    }

    #[test]
    fn validation_reports_malformed_record() {
        let fs = FakeFilesystem::new();
        let root = intent_root(&fs);
        fs.add_file("/repo/intents/broken.toml", "not valid = [");

        let issues = validate_intents(&root, &fs).unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("could not parse intent"));
    }
}
