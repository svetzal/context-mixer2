use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::gateway::filesystem::Filesystem;

/// Directory/file names cmx treats as transient: generated or vendored content
/// that is regenerable from tracked sources (package manifests, lockfiles, the
/// source repo) and is never authored skill content.
///
/// These are ignored both when **checksumming** a skill (so installing its
/// dependencies or running its scripts does not register as drift) and when
/// **copying** a skill (so the canonical home and projected installs stay lean).
const TRANSIENT_NAMES: &[&str] = &["node_modules", "__pycache__", ".git", ".DS_Store"];

/// Whether a directory entry name is transient and should be skipped when
/// checksumming or copying a skill. Matches the [`TRANSIENT_NAMES`] set plus
/// compiled-Python (`*.pyc`) files.
pub(crate) fn is_transient(file_name: &str) -> bool {
    TRANSIENT_NAMES.contains(&file_name)
        || Path::new(file_name).extension().is_some_and(|e| e.eq_ignore_ascii_case("pyc"))
}

/// Recursively collect all non-hidden, non-transient files under `dir` via the
/// given filesystem.
pub(crate) fn collect_files_recursive(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = fs.read_dir(dir)?;

    for entry in entries {
        if entry.file_name.starts_with('.') || is_transient(&entry.file_name) {
            continue;
        }
        if entry.is_dir {
            files.extend(collect_files_recursive(&entry.path, fs)?);
        } else {
            files.push(entry.path);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use std::collections::BTreeSet;

    #[test]
    fn empty_directory_returns_empty_vec() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");
        let result = collect_files_recursive(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn flat_directory_returns_all_file_paths() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/alpha.md", "# alpha");
        fs.add_file("/repo/beta.md", "# beta");
        let result = collect_files_recursive(Path::new("/repo"), &fs).unwrap();
        let paths: BTreeSet<_> = result.into_iter().collect();
        assert!(paths.contains(&PathBuf::from("/repo/alpha.md")));
        assert!(paths.contains(&PathBuf::from("/repo/beta.md")));
    }

    #[test]
    fn nested_directories_collected_recursively() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/agents/agent.md", "# agent");
        fs.add_file("/repo/skills/my-skill/SKILL.md", "# skill");
        let result = collect_files_recursive(Path::new("/repo"), &fs).unwrap();
        let paths: BTreeSet<_> = result.into_iter().collect();
        assert!(paths.contains(&PathBuf::from("/repo/agents/agent.md")));
        assert!(paths.contains(&PathBuf::from("/repo/skills/my-skill/SKILL.md")));
    }

    #[test]
    fn hidden_files_and_dirs_are_skipped() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.hidden-file.md", "hidden");
        fs.add_file("/repo/.hidden-dir/agent.md", "also hidden");
        fs.add_file("/repo/visible.md", "visible");
        let result = collect_files_recursive(Path::new("/repo"), &fs).unwrap();
        let paths: BTreeSet<_> = result.into_iter().collect();
        assert!(paths.contains(&PathBuf::from("/repo/visible.md")));
        assert!(!paths.contains(&PathBuf::from("/repo/.hidden-file.md")));
        assert!(!paths.contains(&PathBuf::from("/repo/.hidden-dir/agent.md")));
    }

    #[test]
    fn transient_dirs_and_files_are_skipped() {
        let fs = FakeFilesystem::new();
        fs.add_file("/skill/SKILL.md", "# skill");
        fs.add_file("/skill/scripts/tool.mjs", "code");
        // Transient: must NOT be collected (would otherwise cause false drift).
        fs.add_file("/skill/scripts/node_modules/dep/index.js", "vendored");
        fs.add_file("/skill/scripts/__pycache__/tool.cpython-312.pyc", "bytecode");
        fs.add_file("/skill/scripts/compiled.pyc", "bytecode");

        let result = collect_files_recursive(Path::new("/skill"), &fs).unwrap();
        let paths: BTreeSet<_> = result.into_iter().collect();
        assert!(paths.contains(&PathBuf::from("/skill/SKILL.md")));
        assert!(paths.contains(&PathBuf::from("/skill/scripts/tool.mjs")));
        assert!(
            !paths.iter().any(|p| p.to_string_lossy().contains("node_modules")),
            "node_modules must be skipped"
        );
        assert!(
            !paths.iter().any(|p| p.to_string_lossy().contains("__pycache__")),
            "__pycache__ must be skipped"
        );
        assert!(
            !paths.contains(&PathBuf::from("/skill/scripts/compiled.pyc")),
            "*.pyc must be skipped"
        );
    }

    #[test]
    fn is_transient_matches_expected_names() {
        assert!(is_transient("node_modules"));
        assert!(is_transient("__pycache__"));
        assert!(is_transient(".git"));
        assert!(is_transient(".DS_Store"));
        assert!(is_transient("tool.cpython-312.pyc"));
        assert!(!is_transient("SKILL.md"));
        assert!(!is_transient("scripts"));
        assert!(!is_transient("package.json"));
    }

    #[test]
    fn non_existent_directory_returns_error() {
        let fs = FakeFilesystem::new();
        let result = collect_files_recursive(Path::new("/does/not/exist"), &fs);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to read directory"), "unexpected: {msg}");
    }
}
