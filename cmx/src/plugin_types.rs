use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Author {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// Same struct as [`Author`] but with a different semantic name for marketplace
/// ownership.
pub type Owner = Author;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceMetadata {
    pub description: String,
    pub version: String,
}

/// The source location of a marketplace plugin entry.
///
/// A bare JSON string is a local relative path (`Local`); a JSON object is a
/// remote source (github, url, npm, etc.) that is not yet supported by the
/// scanner (`Remote`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PluginSource {
    Local(String),
    Remote(serde_json::Value),
}

impl PluginSource {
    pub fn as_local(&self) -> Option<&str> {
        match self {
            PluginSource::Local(s) => Some(s.as_str()),
            PluginSource::Remote(_) => None,
        }
    }

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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MarketplaceEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source: Option<PluginSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Marketplace {
    #[serde(default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MarketplaceMetadata>,
    #[serde(default)]
    pub plugins: Vec<MarketplaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PluginManifest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
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
        assert_eq!(mp.metadata.as_ref().unwrap().version, "1.0.0");
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
