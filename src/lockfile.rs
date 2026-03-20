use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::config;
use crate::types::LockFile;

pub fn lock_path(local: bool) -> Result<PathBuf> {
    if local {
        Ok(PathBuf::from(".context-mixer").join("cmx-lock.json"))
    } else {
        Ok(config::config_dir()?.join("cmx-lock.json"))
    }
}

pub fn load(local: bool) -> Result<LockFile> {
    let path = lock_path(local)?;
    if !path.exists() {
        return Ok(LockFile {
            version: 1,
            packages: BTreeMap::new(),
        });
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let lock: LockFile = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(lock)
}

pub fn save(lock: &LockFile, local: bool) -> Result<()> {
    let path = lock_path(local)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(lock)?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
