//! Crate root for `cmf` (Context Mixer Forge), the publisher tool for
//! authoring the material `cmx` consumes: facets assembled into agents by
//! recipes, plugin scaffolding and validation, and marketplace/manifest
//! generation.
//!
//! Note: `cmf` depends on `cmx` for `plugin_types` — `cmf/src/plugin_types.rs`
//! is a thin re-export shim (`pub use cmx::plugin_types::{...}`), not a
//! second source of truth. The serde types for `plugin.json` and
//! `marketplace.json` live in `cmx/src/plugin_types.rs`.

pub mod cli;
pub mod display;
pub mod facet;
pub mod facet_types;
pub mod manifest;
pub mod marketplace;
pub mod plugin;
pub mod plugin_types;
pub mod recipe;
pub mod repo;
#[cfg(test)]
pub mod test_support;
pub mod validate;
pub mod validation;
