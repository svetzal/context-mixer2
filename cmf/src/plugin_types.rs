use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Author {
    pub name: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceEntry {
    pub name: String,
    pub description: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Marketplace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MarketplaceMetadata>,
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
}
