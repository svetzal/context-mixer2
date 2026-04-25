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
