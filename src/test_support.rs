#[cfg(test)]
pub(crate) fn test_paths() -> crate::paths::ConfigPaths {
    use std::path::PathBuf;
    crate::paths::ConfigPaths::for_test(
        PathBuf::from("/home/testuser"),
        PathBuf::from("/home/testuser/.config/context-mixer"),
    )
}

#[cfg(test)]
pub(crate) fn make_ctx<'a>(
    fs: &'a crate::gateway::fakes::FakeFilesystem,
    git: &'a crate::gateway::fakes::FakeGitClient,
    clock: &'a crate::gateway::fakes::FakeClock,
    paths: &'a crate::paths::ConfigPaths,
) -> crate::context::AppContext<'a> {
    crate::context::AppContext {
        fs,
        git,
        clock,
        paths,
        llm: None,
    }
}
