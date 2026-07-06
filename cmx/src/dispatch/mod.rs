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
pub use set::{handle_set, scope_from};
pub use source::handle_source;

use anyhow::Result;
use serde::Serialize;

pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(crate) fn usage_error(message: &str, example: &str) -> anyhow::Error {
    anyhow::anyhow!("{message}\ntry: {example}")
}
