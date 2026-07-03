//! Production `AppContext` factory for embedding tools.
//!
//! Embedding tools (parite, gilt, hopper, …) construct a [`ProductionContext`]
//! once at startup and use [`ProductionContext::ctx`] to obtain an `AppContext`
//! for every library call, avoiding the need to know about gateway internals.

use anyhow::Result;

use crate::context::AppContext;
use crate::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use crate::paths::ConfigPaths;
use crate::platform::Platform;

/// Owns the production gateway implementations and the resolved paths.
///
/// Constructed once per process via [`from_env`](Self::from_env) and kept alive
/// for the duration of the call — the `AppContext` it vends borrows from it.
pub struct ProductionContext {
    fs: RealFilesystem,
    git: RealGitClient,
    clock: SystemClock,
    paths: ConfigPaths,
}

impl ProductionContext {
    /// Build a production context from the real environment.
    ///
    /// `platform` is the active platform the embedding tool is binding to
    /// (e.g. `Platform::Claude`). Path resolution is derived from the real
    /// home directory.
    pub fn from_env(platform: Platform) -> Result<Self> {
        let paths = ConfigPaths::from_env(platform)?;
        Ok(Self {
            fs: RealFilesystem,
            git: RealGitClient,
            clock: SystemClock,
            paths,
        })
    }

    /// Borrow an `AppContext` for a single library call.
    ///
    /// The returned context does not include an LLM client — embedding tools
    /// do not need LLM-powered analysis.
    pub fn ctx(&self) -> AppContext<'_> {
        AppContext {
            fs: &self.fs,
            git: &self.git,
            clock: &self.clock,
            paths: &self.paths,
            llm: None,
        }
    }
}
