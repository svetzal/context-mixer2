//! Shared test helpers for command dispatch tests, a submodule of
//! `cmx/src/dispatch/mod.rs`.

use chrono::Utc;

use crate::cli::OutputArgs;
use crate::context::AppContext;
use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
use crate::paths::ConfigPaths;
use std::path::PathBuf;

pub(crate) fn test_paths() -> ConfigPaths {
    ConfigPaths::for_test(
        PathBuf::from("/home/testuser"),
        PathBuf::from("/home/testuser/.config/context-mixer"),
    )
}

pub(crate) fn make_test_ctx<'a>(
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

pub(crate) fn fake_trio() -> (FakeFilesystem, FakeGitClient, FakeClock, ConfigPaths) {
    let paths = test_paths();
    (FakeFilesystem::new(), FakeGitClient::new(), FakeClock::at(Utc::now()), paths)
}

pub(crate) fn no_json() -> OutputArgs {
    OutputArgs { json: false }
}
