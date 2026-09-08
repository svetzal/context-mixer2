//! The shared reader of the **intent atlas** (see `CMV.md`, "Three
//! responsibilities" and "The `intent-atlas` crate").
//!
//! The intent atlas is the externally maintained knowledge base cmf composes
//! from and cmv verifies against: the ecosystems it supports, the intent
//! records placed in the directory hierarchy that names their ecosystem, the
//! validators beside those records, and the profiles that name a slice of it
//! for a delivery surface. It lives outside this workspace; this crate is the
//! one reader of its shape that cmf and cmv share, so neither binary depends
//! on the other for it and neither duplicates it.
//!
//! - [`catalog`] scans the records below `intents/` and exposes the ecosystem
//!   qualifiers a catalog key carries and the validators a record declares.
//! - [`profile`] loads and validates a materialization profile.
//! - [`selection`] turns a profile and a scanned catalog into the selected
//!   key set: initial selection, `follow` expansion, downward specialization,
//!   and shadowed-parent removal.
//! - [`sensors`] reads the atlas's `ecosystems.toml`, validates it against
//!   the catalog, and detects a project's ecosystems from it.
//! - [`manifest`] is the compile manifest cmf writes and cmv reads.
//!
//! Rendering the selection into guidance and enforcing the budget stay in
//! cmf; running validators stays in cmv. Every effect goes through cmx-core's
//! gateways, so the same code runs byte-identically against in-memory fakes.
//!
//! This crate is deliberately not part of `cmx-core`: cmx has no use for it,
//! and cmx-core is twinned with a TypeScript port under a conformance suite
//! and released to crates.io and npm in lockstep, so anything placed there
//! would pay a port and a coordinated release for a consumer that does not
//! exist.

#![deny(missing_docs)]

pub mod catalog;
pub mod manifest;
pub mod profile;
pub mod selection;
pub mod sensors;
