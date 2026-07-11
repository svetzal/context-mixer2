use crate::error::Result;
use std::collections::BTreeMap;

use crate::config;
use crate::gateway::{Clock, Filesystem, GitClient, LlmClient};
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::types::{InstallScope, LockFile, SourcesFile};

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

impl<'a> AppContext<'a> {
    /// Return a copy of this context with `paths` replaced.
    pub fn with_paths<'b>(&self, paths: &'b ConfigPaths) -> AppContext<'b>
    where
        'a: 'b,
    {
        AppContext {
            fs: self.fs,
            git: self.git,
            clock: self.clock,
            paths,
            llm: self.llm,
        }
    }

    /// Return a copy of this context with `llm` set to `Some(llm)`.
    #[cfg(feature = "llm")]
    pub fn with_llm<'b>(&self, llm: &'b dyn LlmClient) -> AppContext<'b>
    where
        'a: 'b,
    {
        AppContext {
            fs: self.fs,
            git: self.git,
            clock: self.clock,
            paths: self.paths,
            llm: Some(llm),
        }
    }
}

/// Pre-loaded configuration state — sources file plus both lock files.
///
/// Consolidates the repeated pattern of loading sources + both lock files at
/// the start of every command into a single I/O step.  Command modules call
/// [`LoadedState::load`] once and then pass the plain data to pure logic
/// functions that accept no `&AppContext`.
pub struct LoadedState {
    pub sources: SourcesFile,
    pub locks: BTreeMap<InstallScope, LockFile>,
}

impl LoadedState {
    pub fn load(ctx: &AppContext<'_>) -> Result<Self> {
        let sources = config::load_sources(ctx.fs, ctx.paths)?;
        let locks = lockfile::load_both(ctx.fs, ctx.paths)?;
        Ok(Self { sources, locks })
    }

    /// Return a reference to the lock file for the given scope.
    pub fn lock(&self, scope: InstallScope) -> &LockFile {
        &self.locks[&scope]
    }

    /// Iterate over all scopes and their lock files (global first, then local).
    pub fn scopes(&self) -> impl Iterator<Item = (InstallScope, &LockFile)> {
        self.locks.iter().map(|(s, l)| (*s, l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::lockfile;
    use crate::test_support::{make_lock_entry_builder, test_paths};
    use crate::types::{ArtifactKind, InstallScope, LockFile};
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn make_ctx<'a>(
        fs: &'a FakeFilesystem,
        git: &'a FakeGitClient,
        clock: &'a FakeClock,
        paths: &'a ConfigPaths,
    ) -> AppContext<'a> {
        AppContext {
            fs,
            git,
            clock,
            paths,
            llm: None,
        }
    }

    #[test]
    fn loaded_state_load_empty_fs_returns_defaults() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let ctx = make_ctx(&fs, &git, &clock, &paths);

        let state = LoadedState::load(&ctx).unwrap();
        assert!(state.sources.sources.is_empty());
        assert!(state.lock(InstallScope::Global).packages.is_empty());
        assert!(state.lock(InstallScope::Local).packages.is_empty());
    }

    #[test]
    fn loaded_state_lock_returns_scope_lockfile() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());

        let entry = make_lock_entry_builder(ArtifactKind::Agent, "myrepo", "agents/my-agent.md");
        let mut packages = BTreeMap::new();
        packages.insert("my-agent".to_string(), entry);
        let lock = LockFile {
            version: 1,
            packages,
        };
        lockfile::save(&lock, InstallScope::Global, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let state = LoadedState::load(&ctx).unwrap();

        assert!(state.lock(InstallScope::Global).packages.contains_key("my-agent"));
        assert!(state.lock(InstallScope::Local).packages.is_empty());
    }

    #[test]
    fn loaded_state_scopes_global_first() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let ctx = make_ctx(&fs, &git, &clock, &paths);

        let state = LoadedState::load(&ctx).unwrap();
        let scopes: Vec<InstallScope> = state.scopes().map(|(s, _)| s).collect();
        assert_eq!(scopes, vec![InstallScope::Global, InstallScope::Local]);
    }
}
