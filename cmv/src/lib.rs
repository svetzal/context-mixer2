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
//! The core ([`dispatch`], [`verdict`], [`report`], [`language`], [`config`])
//! is pure over two gateways — cmx-core's `Filesystem` and this crate's own
//! [`process::ProcessRunner`] — so every decision is testable with in-memory
//! fakes. Only `main.rs` and [`process::RealProcessRunner`] touch the OS.
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
pub mod language;
pub mod process;
pub mod report;
pub mod verdict;
