//! Deterministic materialization of structured intent records into agent guidance.
//!
//! Reading the intent atlas — catalog scanning, profile loading, selection,
//! the compile manifest, and the ecosystem sensors — is the `intent-atlas`
//! crate's job, shared with cmv. cmf adds rendering and the budget
//! ([`assembly`]), the ecosystem mismatch warning at `install --local`
//! ([`mismatch`]), and the command grammar ([`cli`]). The atlas modules are
//! re-exported here so cmf's own code
//! and tests keep their `cmf::catalog::…` paths; cmv uses `intent_atlas::`
//! directly and never depends on cmf.

pub mod assembly;
pub mod cli;
pub mod mismatch;

pub use intent_atlas::{catalog, manifest, profile, sensors};
