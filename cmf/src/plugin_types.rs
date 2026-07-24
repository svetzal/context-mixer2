//! Thin re-export shim (`pub use cmx::plugin_types::{...}`); the serde
//! types for plugin.json and marketplace.json now live in
//! `cmx/src/plugin_types.rs` (single source of truth).

pub use cmx::plugin_types::{
    Author, Marketplace, MarketplaceEntry, MarketplaceMetadata, Owner, PluginManifest, PluginSource,
};
