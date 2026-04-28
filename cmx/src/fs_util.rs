use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::gateway::filesystem::Filesystem;

/// Recursively collect all non-hidden files under `dir` via the given filesystem.
pub(crate) fn collect_files_recursive(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = fs
        .read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?;

    for entry in entries {
        if entry.file_name.starts_with('.') {
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
    fn non_existent_directory_returns_error() {
        let fs = FakeFilesystem::new();
        let result = collect_files_recursive(Path::new("/does/not/exist"), &fs);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to read directory"), "unexpected: {msg}");
    }
}
