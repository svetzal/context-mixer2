pub mod adopt;
pub mod artifact;
pub mod config;
pub mod diff;
pub mod info;
pub mod set;
pub mod source;

#[cfg(test)]
pub mod test_support;

pub use adopt::{handle_adopt, handle_unadopt};
pub use artifact::{handle_artifact, handle_install, handle_uninstall, handle_update};
pub use config::{handle_config, handle_home};
pub use diff::handle_diff;
pub use info::handle_info;
pub use set::handle_set;
pub use source::handle_source;

use anyhow::Result;
use serde::Serialize;

use crate::types::InstallScope;

/// Convert the raw `--local` flag onto an [`InstallScope`]. The single
/// conversion point shared by the `set`/`artifact`/`adopt` dispatch modules —
/// only call sites in `cmx/src/dispatch/` and `cmx/src/main.rs` should call
/// this, per the boundary convention described in `crate::flags`.
pub fn scope_from(local: bool) -> InstallScope {
    if local {
        InstallScope::Local
    } else {
        InstallScope::Global
    }
}

pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(crate) fn usage_error(message: &str, example: &str) -> anyhow::Error {
    anyhow::anyhow!("{message}\ntry: {example}")
}
