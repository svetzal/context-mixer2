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
    /// Build a production context bound to the Claude platform.
    ///
    /// This is the one-call default for tools that target Claude Code. It is
    /// equivalent to `from_env(Platform::Claude)`.
    ///
    /// The platform binding determines which lock file and install directory are
    /// used as the *default* for path resolution. It does **not** determine
    /// which platforms a skill installs to — installation targets are resolved
    /// from the cmx config and existing lock files at plan time.
    pub fn claude() -> Result<Self> {
        Self::from_env(Platform::Claude)
    }

    /// Build a production context from the real environment for the given
    /// platform binding.
    ///
    /// `default_platform` controls the default lock file name and install
    /// directory used for path resolution (e.g. which `cmx-lock*.json` file is
    /// the primary one). It does **not** set the config root directory — that is
    /// always `$HOME/.config/context-mixer` — and it does **not** determine
    /// which platforms a skill installs to. Installation targets come from
    /// the plan (resolved from the cmx config and existing lock files).
    ///
    /// For Claude Code tools, prefer [`claude()`](Self::claude) over calling
    /// this directly.
    pub fn from_env(default_platform: Platform) -> Result<Self> {
        let paths = ConfigPaths::from_env(default_platform)?;
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
