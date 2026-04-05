use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

use crate::gateway::filesystem::Filesystem;

pub fn load_json<T>(path: &Path, fs: &dyn Filesystem) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    if !fs.exists(path) {
        return Ok(T::default());
    }
    let content = fs
        .read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let value: T = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(value)
}

pub fn save_json<T>(value: &T, path: &Path, fs: &dyn Filesystem) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)?;
    fs.write(path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
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
    }
}
