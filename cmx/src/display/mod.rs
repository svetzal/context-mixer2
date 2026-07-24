//! Output formatting for all commands; one submodule per command family
//! (see `cmx/src/display/*` and `cmx/src/display/doctor/`).

mod adopt;
mod config;
mod diff;
pub mod doctor;
mod info;
pub mod init;
mod install;
pub mod json;
mod list;
mod outdated;
mod promote;
mod search;
mod sets;
mod source;
mod sync;
mod uninstall;
mod util;
