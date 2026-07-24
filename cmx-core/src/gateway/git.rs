//! The [`GitClient`] gateway trait for cloning and updating git-backed sources.

use crate::error::Result;
use std::path::Path;

/// Abstraction over git operations used by cmx.
///
/// The real implementation delegates to `git` via `std::process::Command`.
/// Tests inject a fake that records calls without running git.
pub trait GitClient {
    /// Clone `url` into `dest`.
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<()>;
    /// Pull the latest changes into the git working copy at `repo_path`.
    fn pull(&self, repo_path: &Path) -> Result<()>;
}
