use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use crate::fs_util;
use crate::types::ArtifactKind;

/// Compute SHA-256 checksum for an agent (single .md file).
pub fn checksum_file(path: &Path) -> Result<String> {
    let content = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(format!("sha256:{:x}", hash))
}

/// Compute SHA-256 checksum for a skill (directory).
/// Hashes all files in sorted order for determinism.
pub fn checksum_dir(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files = fs_util::collect_files(path)?;
    files.sort();

    for file in &files {
        let relative = file.strip_prefix(path).unwrap_or(file);
        hasher.update(relative.to_string_lossy().as_bytes());
        let content =
            fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        hasher.update(&content);
    }

    let hash = hasher.finalize();
    Ok(format!("sha256:{:x}", hash))
}

/// Compute the checksum for an artifact, dispatching to the correct strategy
/// based on its kind: file checksum for agents, directory checksum for skills.
pub fn checksum_artifact(path: &Path, kind: ArtifactKind) -> Result<String> {
    match kind {
        ArtifactKind::Agent => checksum_file(path),
        ArtifactKind::Skill => checksum_dir(path),
    }
}
