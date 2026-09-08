//! Deterministic verification that a repository holds the intents cmf compiled
//! for it (see `CMV.md`).
//!
//! cmv reads the compile manifest cmf wrote for a project, resolves each
//! compiled intent's record in the atlas, runs the validators that
//! match the workspace's ecosystems (detected by the atlas's own sensors),
//! and reports one verdict per intent. It has
//! no `llm` feature, reads no clock, and executes no project code: given the
//! same workspace, manifest, atlas, and `cmv.toml`, its output is
//! byte-identical.
//!
//! The core ([`resolve`], [`pin`], [`dispatch`], [`explain`], [`verdict`],
//! [`report`], [`ecosystems`], [`config`]) is pure over two gateways —
//! cmx-core's `Filesystem` and this crate's own [`process::ProcessRunner`] —
//! so every decision is testable with in-memory fakes. Only `main.rs` and
//! [`process::RealProcessRunner`] touch the OS.
//!
//! The atlas is resolved through the cmx source registry
//! ([`resolve`]) and, when the manifest pins a revision the checkout has moved
//! past, verified at that revision by materializing the pinned tree with
//! `git archive` ([`pin`]). Stale is always computed against the working tree.
//!
//! cmv reads the atlas through the `intent-atlas` crate — the compile
//! manifest ([`intent_atlas::manifest::Manifest`]), the catalog reader
//! ([`intent_atlas::catalog::scan`], [`intent_atlas::catalog::Validator`]),
//! and the sensors ([`intent_atlas::sensors`]) — which cmf shares. It does
//! not depend on cmf.

pub mod cli;
pub mod config;
pub mod dispatch;
pub mod ecosystems;
pub mod explain;
pub mod pin;
pub mod process;
pub mod report;
pub mod resolve;
pub mod verdict;
