//! Generic, atomic JSON-file load/save helpers built on the [`Filesystem`] gateway.
//!
//! [`crate::config`] and [`crate::lockfile`] both build their document-specific
//! load/save functions on [`load_json`] and `save_json` rather than reimplementing
//! parsing and atomic-write logic per document type.

use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

use crate::error::{CmxError, Result};
use crate::gateway::filesystem::Filesystem;

/// Load and parse `path` as JSON into `T`, or return `T::default()` if the file
/// does not exist.
pub fn load_json<T>(path: &Path, fs: &dyn Filesystem) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    if !fs.exists(path) {
        return Ok(T::default());
    }
    let content = fs.read_to_string(path)?;
    serde_json::from_str(&content).map_err(|source| CmxError::Json {
        context: format!("Failed to parse {}", path.display()),
        path: path.to_path_buf(),
        source,
    })
}

/// Return the sibling temporary path used during an atomic write of `path`.
///
/// The temp path is `path` with `.tmp` appended to the file name, so it sits
/// in the same directory and can be renamed atomically onto the target.
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Write `value` as pretty-printed JSON to `path` atomically.
///
/// The JSON is first written to a sibling `.tmp` file in the same directory,
/// then renamed onto `path`.  This ensures that a partially-written or failed
/// write never corrupts an existing file: the rename only happens after the
/// write succeeds.
pub fn save_json<T>(value: &T, path: &Path, fs: &dyn Filesystem) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(value).map_err(|source| CmxError::Json {
        context: "Failed to serialize JSON".to_string(),
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = tmp_path(path);
    fs.write(&tmp, &content)?;
    fs.rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct TestData {
        value: String,
        count: u32,
    }

    #[test]
    fn load_json_returns_default_when_file_absent() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/nonexistent/data.json");
        let result: TestData = load_json(&path, &fs).unwrap();
        assert_eq!(result, TestData::default());
    }

    #[test]
    fn load_json_round_trip_save_then_load() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/data.json");
        let data = TestData {
            value: "hello".to_string(),
            count: 42,
        };
        save_json(&data, &path, &fs).unwrap();
        let loaded: TestData = load_json(&path, &fs).unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn load_json_returns_error_on_malformed_json() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/data.json");
        fs.add_file(path.clone(), "not json {{{{");
        let result: Result<TestData> = load_json(&path, &fs);
        assert!(result.is_err());
        // Typed: CmxError::Json
        assert!(matches!(result.unwrap_err(), CmxError::Json { .. }));
    }

    #[test]
    fn tmp_path_appends_tmp_suffix() {
        let path = PathBuf::from("/config/data.json");
        assert_eq!(tmp_path(&path), PathBuf::from("/config/data.json.tmp"));
    }

    #[test]
    fn save_json_failed_write_leaves_existing_file_intact() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/data.json");
        let original = TestData {
            value: "original".to_string(),
            count: 1,
        };
        // Write the original file first
        save_json(&original, &path, &fs).unwrap();

        // Now cause the temp-file write to fail
        fs.set_fail_on_write(tmp_path(&path));

        let new_data = TestData {
            value: "new".to_string(),
            count: 2,
        };
        let result = save_json(&new_data, &path, &fs);
        assert!(result.is_err(), "expected Err when temp write fails");

        // The existing file should be unmodified
        let loaded: TestData = load_json(&path, &fs).unwrap();
        assert_eq!(loaded, original, "existing file should be intact after failed temp write");
    }

    #[test]
    fn save_json_failed_rename_leaves_existing_file_intact() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/config/data.json");
        let original = TestData {
            value: "original".to_string(),
            count: 1,
        };
        // Write the original file first
        save_json(&original, &path, &fs).unwrap();

        // Cause the rename (onto the final path) to fail
        fs.set_fail_on_rename(path.clone());

        let new_data = TestData {
            value: "new".to_string(),
            count: 2,
        };
        let result = save_json(&new_data, &path, &fs);
        assert!(result.is_err(), "expected Err when rename fails");

        // The existing file should be unmodified
        let loaded: TestData = load_json(&path, &fs).unwrap();
        assert_eq!(loaded, original, "existing file should be intact after failed rename");
    }
}
