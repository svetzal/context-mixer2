use anyhow::Result;
use std::path::Path;

/// Abstraction over git operations used by cmx.
///
/// The real implementation delegates to `git` via `std::process::Command`.
/// Tests inject a fake that records calls without running git.
pub trait GitClient {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<()>;
    fn pull(&self, repo_path: &Path) -> Result<()>;
}
