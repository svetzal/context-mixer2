//! Formatting for plugin lists, recipes, facets, manifests, and validation
//! results; submodules: `cmf/src/display/facet.rs`,
//! `cmf/src/display/manifest.rs`, `cmf/src/display/plugin.rs`,
//! `cmf/src/display/status.rs`, `cmf/src/display/validation.rs`.

mod facet;
mod manifest;
mod plugin;
mod status;
mod validation;

pub use status::status_report;
