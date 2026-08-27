//! Suppress the `$HOME` local/global install-dir alias case at cmx's
//! command-orchestration boundary, without changing the shared cmx-core path
//! semantics.

use std::path::Path;

use crate::gateway::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::{ArtifactKind, InstallScope};

/// Whether both paths exist and resolve to the same physical location.
pub(crate) fn paths_alias(local_path: &Path, global_path: &Path, fs: &dyn Filesystem) -> bool {
    if !fs.exists(local_path) || !fs.exists(global_path) {
        return false;
    }

    match (fs.canonicalize(local_path), fs.canonicalize(global_path)) {
        (Ok(local), Ok(global)) => local == global,
        _ => false,
    }
}

/// Whether the current working directory's local install dir for `kind` is the
/// same physical directory as the global install dir for this platform.
pub(crate) fn local_install_dir_aliases_global(
    paths: &ConfigPaths,
    kind: ArtifactKind,
    fs: &dyn Filesystem,
) -> bool {
    let Some(local_dir) = paths.install_dir(kind, InstallScope::Local) else {
        return false;
    };
    let Some(global_dir) = paths.install_dir(kind, InstallScope::Global) else {
        return false;
    };
    paths_alias(&local_dir, &global_dir, fs)
}

#[cfg(test)]
mod tests {
    use super::{local_install_dir_aliases_global, paths_alias};
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::test_paths;
    use crate::types::ArtifactKind;

    #[test]
    fn paths_alias_is_false_when_either_path_missing() {
        let fs = FakeFilesystem::new();
        assert!(!paths_alias(
            std::path::Path::new(".claude/agents"),
            std::path::Path::new("/home/testuser/.claude/agents"),
            &fs,
        ));
    }

    #[test]
    fn local_install_dir_aliases_global_is_false_when_paths_do_not_exist() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        assert!(!local_install_dir_aliases_global(&paths, ArtifactKind::Skill, &fs));
    }
}
