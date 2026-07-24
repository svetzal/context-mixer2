//! Serde types for plugin.json and marketplace.json (single source of
//! truth lifted from cmf).

use serde::{Deserialize, Serialize};

/// Author (or owner) identity as recorded in `plugin.json`/`marketplace.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Author {
    /// Display name of the author.
    #[serde(default)]
    pub name: String,
    /// Contact email of the author.
    #[serde(default)]
    pub email: String,
}

/// Same struct as [`Author`] but with a different semantic name for marketplace
/// ownership.
pub type Owner = Author;

/// Optional descriptive block on a `marketplace.json`. Both fields are
/// optional: third-party marketplaces (e.g. upstream Claude Code plugin repos)
/// frequently provide only a `description`, or omit the block entirely. cmx
/// must ingest those files without aborting, so neither field is required.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceMetadata {
    /// Human-readable description of the marketplace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Marketplace manifest version, if the publisher declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The source location of a marketplace plugin entry.
///
/// A bare JSON string is a local relative path (`Local`); a JSON object is a
/// remote source (github, url, npm, etc.) that is not yet supported by the
/// scanner (`Remote`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PluginSource {
    /// A relative path within the marketplace repo, e.g. `./plugins/foo`.
    Local(String),
    /// A remote source descriptor (e.g. `{"source": "github", ...}`), not yet
    /// supported by the scanner but preserved for round-tripping.
    Remote(serde_json::Value),
}

impl PluginSource {
    /// Return the local relative path, or `None` if this is a `Remote` source.
    pub fn as_local(&self) -> Option<&str> {
        match self {
            PluginSource::Local(s) => Some(s.as_str()),
            PluginSource::Remote(_) => None,
        }
    }

    /// Short name for this source's kind: `"local"`, or the remote source's
    /// declared `source` field (e.g. `"github"`), falling back to `"unknown"`.
    pub fn source_type_name(&self) -> &str {
        match self {
            PluginSource::Local(_) => "local",
            PluginSource::Remote(v) => {
                v.get("source").and_then(|s| s.as_str()).unwrap_or("unknown")
            }
        }
    }
}

impl std::fmt::Display for PluginSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginSource::Local(s) => write!(f, "{s}"),
            PluginSource::Remote(v) => write!(f, "{v}"),
        }
    }
}

/// One plugin's listing within a `marketplace.json`'s `plugins` array.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MarketplaceEntry {
    /// Plugin name.
    #[serde(default)]
    pub name: String,
    /// Human-readable description of the plugin.
    #[serde(default)]
    pub description: String,
    /// Where the plugin's files live, relative to the marketplace or a remote
    /// location; `None` when only `agents`/`skills` arrays are declared.
    #[serde(default)]
    pub source: Option<PluginSource>,
    /// Optional publisher-assigned category, e.g. `"development"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Explicit list of agent artifact paths, when not inferred from `source`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    /// Explicit list of skill artifact paths, when not inferred from `source`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

/// Deserialized form of a repo's `marketplace.json` — the plugin listing
/// consumed by `cmx source browse`/`cmx set create --from-plugin`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Marketplace {
    /// Name of the marketplace.
    #[serde(default)]
    pub name: String,
    /// Marketplace owner/maintainer identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    /// Optional descriptive metadata block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MarketplaceMetadata>,
    /// Plugins listed by this marketplace.
    #[serde(default)]
    pub plugins: Vec<MarketplaceEntry>,
}

/// Deserialized form of a plugin's own `plugin.json` manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PluginManifest {
    /// Plugin name.
    pub name: String,
    /// Plugin version, if the publisher declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Human-readable description of the plugin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Plugin author identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    /// SPDX license identifier, if declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Free-text search keywords.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_marketplace_json() {
        let json = r#"{
            "name": "svetzal-guidelines",
            "owner": { "name": "Stacey Vetzal", "email": "stacey@vetzal.com" },
            "metadata": { "description": "Curated guidelines", "version": "1.0.0" },
            "plugins": [
                {
                    "name": "rust-craft",
                    "description": "Rust development guidelines",
                    "source": "./plugins/rust-craft",
                    "category": "development"
                }
            ]
        }"#;
        let mp: Marketplace = serde_json::from_str(json).unwrap();
        assert_eq!(mp.name, "svetzal-guidelines");
        assert_eq!(mp.owner.as_ref().unwrap().name, "Stacey Vetzal");
        assert_eq!(mp.metadata.as_ref().unwrap().version.as_deref(), Some("1.0.0"));
        assert_eq!(mp.plugins.len(), 1);
        assert_eq!(mp.plugins[0].name, "rust-craft");
        assert_eq!(mp.plugins[0].category.as_deref(), Some("development"));
        assert_eq!(
            mp.plugins[0].source.as_ref().and_then(|s| s.as_local()),
            Some("./plugins/rust-craft")
        );

        // Round-trip: serialize and deserialize should produce the same value
        let serialized = serde_json::to_string_pretty(&mp).unwrap();
        let round_tripped: Marketplace = serde_json::from_str(&serialized).unwrap();
        assert_eq!(mp, round_tripped);
    }

    #[test]
    fn deserialize_plugin_json() {
        let json = r#"{
            "name": "rust-craft",
            "version": "1.0.0",
            "description": "Rust development guidelines",
            "author": { "name": "Stacey Vetzal", "email": "stacey@vetzal.com" },
            "license": "MIT",
            "keywords": ["rust", "development"]
        }"#;
        let pm: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(pm.name, "rust-craft");
        assert_eq!(pm.version.as_deref(), Some("1.0.0"));
        assert_eq!(pm.author.as_ref().unwrap().email, "stacey@vetzal.com");
        assert_eq!(pm.keywords, vec!["rust", "development"]);

        // Round-trip
        let serialized = serde_json::to_string_pretty(&pm).unwrap();
        let round_tripped: PluginManifest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(pm, round_tripped);
    }

    #[test]
    fn marketplace_default_is_empty() {
        let mp = Marketplace::default();
        assert_eq!(mp.name, "");
        assert!(mp.owner.is_none());
        assert!(mp.metadata.is_none());
        assert!(mp.plugins.is_empty());
    }

    #[test]
    fn plugin_source_object_deserializes_as_remote() {
        let json = r#"{
            "name": "remote-plugin",
            "description": "A plugin",
            "source": {"source": "url", "url": "https://example.com"}
        }"#;
        let entry: MarketplaceEntry = serde_json::from_str(json).unwrap();
        assert!(entry.source.as_ref().and_then(|s| s.as_local()).is_none());
        assert_eq!(entry.source.as_ref().map(PluginSource::source_type_name), Some("url"));
    }

    #[test]
    fn plugin_entry_with_explicit_arrays_no_source() {
        let json = r#"{
            "name": "my-plugin",
            "agents": ["./agents/my-agent.md"],
            "skills": ["./skills/my-skill"]
        }"#;
        let entry: MarketplaceEntry = serde_json::from_str(json).unwrap();
        assert!(entry.source.is_none());
        assert_eq!(entry.agents, vec!["./agents/my-agent.md"]);
        assert_eq!(entry.skills, vec!["./skills/my-skill"]);
    }

    #[test]
    fn local_source_serializes_as_bare_string() {
        let entry = MarketplaceEntry {
            name: "test".to_string(),
            description: "Test plugin".to_string(),
            source: Some(PluginSource::Local("./plugins/test".to_string())),
            ..Default::default()
        };
        let serialized = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(v["source"], serde_json::Value::String("./plugins/test".to_string()));
        assert!(v.get("agents").is_none());
        assert!(v.get("skills").is_none());
    }
}
