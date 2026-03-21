use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Recursively collect all non-hidden files under `dir`, skipping entries
/// whose name starts with `.` (e.g. `.DS_Store`, `.git`, `.gitignore`).
///
/// Files are returned in an unspecified order; callers should sort if
/// determinism is required.
pub fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip dotfiles and dot-directories (.DS_Store, .gitignore, etc.)
        if let Some(name) = path.file_name()
            && name.to_string_lossy().starts_with('.')
        {
            continue;
        }

        if path.is_dir() {
            files.extend(collect_files(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}
