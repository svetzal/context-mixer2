//! Materialization profile schema and loading.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cmx_core::gateway::Filesystem;
use serde::Deserialize;

/// Output delivery surface.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// Custom agent guidance.
    Agent,
    /// Situational skill guidance.
    Skill,
}

/// Reproducible request for one assembled artifact.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    /// Stable profile and default artifact name.
    pub id: String,
    /// Optional artifact name override.
    pub name: Option<String>,
    /// Artifact version recorded by cmx-core.
    #[serde(default = "default_version")]
    pub version: String,
    /// Trigger description or concise agent purpose.
    pub description: String,
    /// Delivery surface.
    pub surface: Surface,
    /// Approximate maximum context tokens, estimated at four characters each.
    pub budget_tokens: usize,
    /// Initial selection rules.
    pub select: Selection,
    /// Graph traversal rules.
    #[serde(default)]
    pub graph: Graph,
    /// Content extraction rules.
    #[serde(default)]
    pub content: Content,
}

impl Profile {
    /// Installed artifact name.
    pub fn artifact_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Initial intent selection rules.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Selection {
    /// Exact intent keys.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Accepted categories.
    #[serde(default)]
    pub categories: Vec<String>,
    /// At least one required tag.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Graph expansion controls.
#[derive(Debug, Clone, Deserialize)]
pub struct Graph {
    /// Relationship types to follow.
    #[serde(default)]
    pub follow: Vec<String>,
    /// Maximum number of graph edges beyond the initial selection.
    #[serde(default)]
    pub max_related_depth: usize,
    /// Drop generalized parents when a selected specialization exists.
    #[serde(default)]
    pub prefer_specializations: bool,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            follow: Vec::new(),
            max_related_depth: 0,
            prefer_specializations: true,
        }
    }
}

/// Fields to include in the delivered artifact.
#[derive(Debug, Clone, Deserialize)]
pub struct Content {
    /// Optional sections: `guidance`, `rationale`, and `evidence`.
    #[serde(default = "default_include")]
    pub include: Vec<String>,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            include: default_include(),
        }
    }
}

fn default_include() -> Vec<String> {
    vec!["guidance".to_string(), "evidence".to_string()]
}
fn default_version() -> String {
    "0.1.0".to_string()
}

/// Load a profile from an explicit path or `<root>/profiles/<name>.toml`.
pub fn load(root: &Path, requested: &Path, fs: &dyn Filesystem) -> Result<(Profile, PathBuf)> {
    let path = if fs.is_file(requested) {
        requested.to_path_buf()
    } else {
        let mut named = root.join("profiles").join(requested);
        if named.extension().is_none() {
            named.set_extension("toml");
        }
        named
    };
    if !fs.is_file(&path) {
        bail!("profile not found: {}", path.display());
    }
    let raw = fs.read_to_string(&path)?;
    let profile: Profile = toml::from_str(&raw)
        .with_context(|| format!("could not parse profile {}", path.display()))?;
    validate(&profile)?;
    Ok((profile, path))
}

fn validate(profile: &Profile) -> Result<()> {
    if profile.id.trim().is_empty() || profile.description.trim().is_empty() {
        bail!("profile id and description must not be empty");
    }
    if profile.budget_tokens == 0 {
        bail!("profile budget_tokens must be greater than zero");
    }
    if profile.select.keys.is_empty()
        && (profile.select.categories.is_empty() || profile.select.tags.is_empty())
    {
        bail!("profile must select exact keys or combine categories with tags");
    }
    for relation in &profile.graph.follow {
        if !matches!(relation.as_str(), "specializes" | "related-to") {
            bail!("unsupported graph relationship {relation:?}");
        }
    }
    for section in &profile.content.include {
        if !matches!(section.as_str(), "guidance" | "rationale" | "evidence") {
            bail!("unsupported content section {section:?}");
        }
    }
    Ok(())
}
