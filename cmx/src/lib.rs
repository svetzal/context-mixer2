pub mod adopt;
pub mod cli;
pub mod cmx_config;
pub(crate) mod codex_agent;
pub(crate) mod copy;
#[cfg(feature = "llm")]
pub mod diff;
pub mod display;
pub mod doctor;
pub mod info;
pub mod install;
pub mod list;
pub mod outdated;
pub mod partition;
pub mod plugin_types;
pub mod promote;
pub mod scan;
pub(crate) mod scan_marketplace;
pub mod search;
pub mod source;
pub(crate) mod source_iter;
pub mod source_update;
pub mod sync;
pub mod table;
#[cfg(feature = "llm")]
pub(crate) mod text_diff;
pub mod uninstall;

// Modules extracted to cmx-core — re-exported here to preserve all existing
// `crate::` paths used throughout this crate's modules and tests.
pub use cmx_core::artifact_status;
pub use cmx_core::checksum;
pub use cmx_core::config;
pub use cmx_core::context;
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
