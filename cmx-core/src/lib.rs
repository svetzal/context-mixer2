//! Embeddable core for installing and tracking agent skills across AI-coding-assistant
//! platforms (Claude, Codex, Cursor, Copilot, and more — see [`platform`]).
//!
//! CLI tools that ship a companion skill use this crate instead of hand-rolling file
//! copies into hard-wired, per-platform paths. The primary entry point is
//! [`skill_install::SkillInstaller`], which implements a **plan → apply** lifecycle
//! mirrored across every operation:
//!
//! - `plan` computes a dry-run install plan (what would be written, updated, skipped,
//!   or refused) without touching disk.
//! - `apply` executes a previously computed plan, re-checking that the bundle has not
//!   changed since `plan()` was called, and writes exactly what the plan reported.
//! - `status` reports per-platform install/tracked/drift state.
//! - `remove` deletes installed files and clears the corresponding lock entries.
//!
//! All I/O — filesystem, git, the system clock, and (optionally) an LLM gateway — is
//! reached only through the [`gateway`] traits, bundled together in [`context::AppContext`].
//! Production code builds one context per process via [`production::ProductionContext`];
//! tests substitute in-memory fakes ([`gateway::fakes`], available under the
//! `test-support` feature) so the same plan/apply/status/remove logic runs against a
//! fake filesystem with no real I/O.
//!
//! # Feature flags
//!
//! - `llm` — enables the [`gateway::LlmClient`] gateway and the real mojentic-backed
//!   implementation, for tools that want LLM-powered diff analysis. Off by default;
//!   pulls in tokio and mojentic when enabled.
//! - `test-support` — exposes the `test_support` module and [`gateway::fakes`] (in-memory
//!   fakes for `Filesystem`, `GitClient`, `Clock`, and `LlmClient`) so embedding crates
//!   can exercise their integration without touching the real filesystem. Enable it
//!   from `[dev-dependencies]`, since Cargo features unify across the dependency graph.
//!
//! # The SPEC/conformance contract
//!
//! cmx-core has a TypeScript twin, `cmx-core-ts`, that must stay behaviorally
//! synchronized with this crate. `cmx-core/SPEC.md` is the language-neutral contract
//! (lockfile format, checksum algorithm, frontmatter reconciliation, the version-guard
//! decision table, target resolution) that both ports must satisfy byte-for-byte where
//! noted; `cmx-core/conformance/` holds the shared golden fixtures that pin it. This
//! crate runs them via the `conformance` module (under `cargo test`); the TypeScript port runs
//! the same fixtures via `bun test`. A behavior change here is only complete once the
//! shared SPEC/fixtures are updated and both ports pass.
//!
//! # Example
//!
//! ```no_run
//! use cmx_core::production::ProductionContext;
//! use cmx_core::skill_install::{BundledSkill, Scope, SkillInstaller, ToolIdentity};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // The bundled SKILL.md needs no version of its own — the installer stamps
//!     // `metadata.version` from the ToolIdentity below at install time.
//!     let skill = BundledSkill::single_md("---\nname: mytool\n---\n# My skill\n");
//!     let installer = SkillInstaller::new(ToolIdentity::new("mytool", "1.2.0"));
//!     let prod_ctx = ProductionContext::claude()?;
//!     let ctx = prod_ctx.ctx();
//!     let plan = installer.plan(&skill, Scope::Global, false, &ctx)?;
//!     println!("{plan}"); // dry-run: names every file and destination
//!     let report = installer.apply(&skill, &plan, &ctx)?;
//!     println!("{report}"); // summary: platform, action, destination, version
//!     Ok(())
//! }
//! ```

pub mod artifact_remove;
pub mod artifact_status;
pub mod checksum;
pub mod config;
pub mod context;
pub mod error;
pub mod error_summary;
pub mod frontmatter;
pub mod fs_util;
pub mod gateway;
pub mod json_file;
pub mod lockfile;
pub mod paths;
pub mod platform;
pub mod platform_iter;
pub mod production;
pub mod skill_fs;
pub mod skill_install;
pub mod targets;
pub mod types;

#[cfg(any(test, feature = "test-support"))]
pub mod conformance;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
