//! # cmx — Context Mixer
//!
//! Crate root for the `cmx` package manager, which manages the lifecycle of
//! curated agentic context — portable agent definitions and composable
//! skills — across AI coding assistants, resting on two pillars of equal
//! weight (see `CHARTER.md`):
//!
//! 1. **Marketplace distribution** — git-backed plugin marketplaces with a
//!    standard manifest format, versioned, checksummed, and tracked through
//!    install, update, and deprecation.
//! 2. **Cross-platform curation and reconciliation** — a tool-neutral
//!    canonical home for hand-authored private artifacts, projected to every
//!    platform in use, with drift detection, promotion of in-place edits
//!    back to the canonical copy, and syncing of copies that have diverged
//!    across platforms.
//!
//! This file is the crate root for both the `cmx` binary (`cmx/src/main.rs`)
//! and a library target: it re-exports every public module, including a set
//! of modules whose *actual source* lives in the `cmx-core` crate.
//!
//! Note: the following modules are re-exported from `cmx-core`, not defined
//! here: `artifact_status`, `checksum`, `config`, `context`, `error_summary`,
//! `fs_util`, `gateway`, `json_file`, `lockfile`, `paths`, `platform`,
//! `platform_iter`, `targets`, `types`. Edits to any of these belong in
//! `cmx-core/src/`, not `cmx/src/`. Creating a file under `cmx/src/` with the
//! same name as one of these re-exports (e.g. a new `paths` module) would
//! silently shadow the re-export and is a mistake — see the Architecture
//! section of `AGENTS.md` for the full module map.

pub mod adopt;
pub mod cli;
pub mod cmx_config;
pub(crate) mod codex_agent;
pub mod completions;
pub(crate) mod copy;
pub mod diff;
pub mod dispatch;
pub mod display;
pub mod doctor;
pub mod error;
pub mod flags;
pub mod info;
pub mod init;
pub mod install;
pub mod list;
pub mod outdated;
pub mod partition;
pub mod platform_copies;
pub mod plugin_types;
pub mod promote;
pub mod scan;
pub(crate) mod scan_marketplace;
pub mod search;
pub mod sets;
pub mod source;
pub(crate) mod source_iter;
pub mod source_update;
pub mod suggestions;
pub mod sync;
pub mod table;
pub(crate) mod text_diff;
pub mod uninstall;

// Modules extracted to cmx-core — re-exported here to preserve all existing
// `crate::` paths used throughout this crate's modules and tests.
pub use cmx_core::artifact_remove;
pub use cmx_core::artifact_status;
pub use cmx_core::checksum;
pub use cmx_core::config;
pub use cmx_core::context;
pub use cmx_core::error_summary;
pub use cmx_core::fs_util;
pub use cmx_core::gateway;
pub use cmx_core::json_file;
pub use cmx_core::lockfile;
pub use cmx_core::paths;
pub use cmx_core::platform;
pub use cmx_core::platform_iter;
pub use cmx_core::targets;
pub use cmx_core::types;

#[cfg(test)]
pub(crate) use cmx_core::test_support;
