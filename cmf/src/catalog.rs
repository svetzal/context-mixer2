//! Read-only discovery of TOML intent records.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cmx_core::gateway::Filesystem;
use serde::Deserialize;

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
    /// Human-readable evidence expectation.
    pub description: String,
    /// Whether the evidence is mandatory.
    #[serde(default)]
    pub required: bool,
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
