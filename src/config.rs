use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::types::{CmxConfig, SourceEntry, SourceType, SourcesFile};

pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".config").join("context-mixer"))
}

pub fn sources_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("sources.json"))
}

pub fn git_clones_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("sources"))
}

pub fn load_sources() -> Result<SourcesFile> {
    let path = sources_path()?;
    if !path.exists() {
        return Ok(SourcesFile::default());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let sources: SourcesFile = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(sources)
}

pub fn save_sources(sources: &SourcesFile) -> Result<()> {
    let path = sources_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(sources)?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config() -> Result<CmxConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(CmxConfig::default());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: CmxConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(config)
}

pub fn save_config(config: &CmxConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn resolve_local_path(entry: &SourceEntry) -> PathBuf {
    match entry.source_type {
        SourceType::Local => entry.path.clone().unwrap_or_default(),
        SourceType::Git => entry.local_clone.clone().unwrap_or_default(),
    }
}
