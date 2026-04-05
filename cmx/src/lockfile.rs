use anyhow::Result;
use std::path::Path;

use crate::gateway::filesystem::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::LockFile;

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem + ConfigPaths)
// ---------------------------------------------------------------------------

/// Load a `LockFile` from an explicit path via the given filesystem.
/// Returns a default (empty) lock file if the path does not exist.
pub fn load_from_with(path: &Path, fs: &dyn Filesystem) -> Result<LockFile> {
    crate::json_file::load_json(path, fs)
}

/// Save a `LockFile` to an explicit path via the given filesystem,
/// creating parent directories as needed.
pub fn save_to_with(lock: &LockFile, path: &Path, fs: &dyn Filesystem) -> Result<()> {
    crate::json_file::save_json(lock, path, fs)
}

pub fn load_with(local: bool, fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<LockFile> {
    let path = paths.lock_path(local);
    load_from_with(&path, fs)
}

pub fn save_with(
    lock: &LockFile,
    local: bool,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    let path = paths.lock_path(local);
    save_to_with(lock, &path, fs)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{sample_lock_file, test_paths};
    use std::path::PathBuf;

    // --- load_from_with ---

    #[test]
    fn load_from_returns_empty_when_path_absent() {
        let fs = FakeFilesystem::new();
        let lock = load_from_with(Path::new("/nonexistent/cmx-lock.json"), &fs).unwrap();
        assert!(lock.packages.is_empty());
        assert_eq!(lock.version, 1);
    }

    #[test]
    fn load_from_parses_valid_json() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/cmx-lock.json");
        let json = serde_json::to_string(&sample_lock_file()).unwrap();
        fs.add_file(path.clone(), json);
        let lock = load_from_with(&path, &fs).unwrap();
        assert!(lock.packages.contains_key("my-agent"));
    }

    #[test]
    fn load_from_returns_error_on_malformed_json() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/cmx-lock.json");
        fs.add_file(path.clone(), "not json");
        assert!(load_from_with(&path, &fs).is_err());
    }

    // --- save_to_with ---

    #[test]
    fn save_to_creates_parent_dirs_and_writes() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/context-mixer/cmx-lock.json");
        save_to_with(&sample_lock_file(), &path, &fs).unwrap();
        assert!(fs.file_exists(&path));
    }

    // --- round-trip via load_with / save_with ---

    #[test]
    fn save_and_load_with_round_trip() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let lock = sample_lock_file();
        save_with(&lock, false, &fs, &paths).unwrap();
        let loaded = load_with(false, &fs, &paths).unwrap();
        assert_eq!(loaded.packages.len(), 1);
        let entry = loaded.packages.get("my-agent").unwrap();
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.source_checksum, "sha256:abc123");
    }
}
