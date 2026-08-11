//! Formatting for intent and plugin lists, manifests, status, and validation
//! results; submodules: `cmf/src/display/intent.rs`,
//! `cmf/src/display/manifest.rs`, `cmf/src/display/plugin.rs`,
//! `cmf/src/display/status.rs`, `cmf/src/display/validation.rs`.

mod intent;
mod manifest;
mod plugin;
mod status;
mod validation;

pub use status::status_report;
