//! Deterministic verification that a repository holds the intents cmf compiled
//! for it (see `CMV.md`).
//!
//! cmv reads the compile manifest cmf wrote for a project, resolves each
//! compiled intent's record in the knowledge base, runs the validators that
//! match the workspace's languages, and reports one verdict per intent. It has
//! no `llm` feature, reads no clock, and executes no project code: given the
//! same workspace, manifest, knowledge base, and `cmv.toml`, its output is
//! byte-identical.
//!
//! The core ([`resolve`], [`pin`], [`dispatch`], [`explain`], [`verdict`],
//! [`report`], [`language`], [`config`]) is pure over two gateways —
//! cmx-core's `Filesystem` and this crate's own [`process::ProcessRunner`] —
//! so every decision is testable with in-memory fakes. Only `main.rs` and
//! [`process::RealProcessRunner`] touch the OS.
//!
//! The knowledge base is resolved through the cmx source registry
//! ([`resolve`]) and, when the manifest pins a revision the checkout has moved
//! past, verified at that revision by materializing the pinned tree with
//! `git archive` ([`pin`]). Stale is always computed against the working tree.
//!
//! cmv depends on the `cmf` crate for [`cmf::manifest::Manifest`] and the
//! catalog reader ([`cmf::catalog::scan`], [`cmf::catalog::Validator`]). This
//! is an interim arrangement: `CMV.md`'s "Shared selection" section leaves open
//! whether catalog and manifest types move into a crate both binaries depend
//! on, or into `cmx-core`. Until that is decided, the verifier reuses the
//! materializer's types directly rather than duplicating them.

pub mod cli;
pub mod config;
pub mod dispatch;
pub mod explain;
pub mod language;
pub mod pin;
pub mod process;
pub mod report;
pub mod resolve;
pub mod verdict;
