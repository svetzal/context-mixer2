//! Crate root for `cmf` (Context Mixer Forge), the compiler and publisher for
//! intent-based agentic guidance consumed by `cmx`.
//!
//! Note: `cmf` depends on `cmx` for `plugin_types` — `cmf/src/plugin_types.rs`
//! is a thin re-export shim (`pub use cmx::plugin_types::{...}`), not a
//! second source of truth. The serde types for `plugin.json` and
//! `marketplace.json` live in `cmx/src/plugin_types.rs`.

pub mod cli;
pub mod display;
pub mod intent;
pub mod manifest;
pub mod marketplace;
pub mod plugin;
pub mod plugin_types;
pub mod repo;
#[cfg(test)]
pub mod test_support;
pub mod validate;
pub mod validation;
