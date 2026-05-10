use anyhow::Result;

use crate::config;
use crate::gateway::{Clock, Filesystem, GitClient, LlmClient};
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::types::{LockFile, SourcesFile};

/// Bundles all I/O gateway dependencies for a command invocation.
///
/// Production code constructs one `AppContext` in `main` with real
/// implementations and passes it down.  Tests construct it with fakes.
pub struct AppContext<'a> {
    pub fs: &'a dyn Filesystem,
    pub git: &'a dyn GitClient,
    pub clock: &'a dyn Clock,
    pub paths: &'a ConfigPaths,
    pub llm: Option<&'a dyn LlmClient>,
}

/// Pre-loaded configuration state — sources file plus both lock files.
///
/// Consolidates the repeated pattern of loading sources + both lock files at
/// the start of every command into a single I/O step.  Command modules call
/// [`LoadedState::load`] once and then pass the plain data to pure logic
/// functions that accept no `&AppContext`.
pub struct LoadedState {
    pub sources: SourcesFile,
    pub global_lock: LockFile,
    pub local_lock: LockFile,
}

impl LoadedState {
    pub fn load(ctx: &AppContext<'_>) -> Result<Self> {
        let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
        let (global_lock, local_lock) = lockfile::load_both_with(ctx.fs, ctx.paths)?;
        Ok(Self {
            sources,
            global_lock,
            local_lock,
        })
    }
}
