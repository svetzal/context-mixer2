use crate::gateway::{Clock, Filesystem, GitClient, LlmClient};
use crate::paths::ConfigPaths;

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
