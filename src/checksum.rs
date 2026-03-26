use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::gateway::filesystem::Filesystem;
use crate::types::ArtifactKind;

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem)
// ---------------------------------------------------------------------------

/// Compute SHA-256 checksum for an agent (single .md file) via the given filesystem.
pub fn checksum_file_with(path: &Path, fs: &dyn Filesystem) -> Result<String> {
    let content = fs.read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(format!("sha256:{}", hex_encode(&hash)))
}

/// Compute SHA-256 checksum for a skill (directory) via the given filesystem.
/// Hashes all files in sorted order for determinism.
pub fn checksum_dir_with(path: &Path, fs: &dyn Filesystem) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files = collect_files_with(path, fs)?;
    files.sort();

    for file in &files {
        let relative = file.strip_prefix(path).unwrap_or(file);
        hasher.update(relative.to_string_lossy().as_bytes());
        let content =
            fs.read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        hasher.update(&content);
    }

    let hash = hasher.finalize();
    Ok(format!("sha256:{}", hex_encode(&hash)))
}

/// Compute the checksum for an artifact, dispatching to the correct strategy
/// based on its kind: file checksum for agents, directory checksum for skills.
pub fn checksum_artifact_with(
    path: &Path,
    kind: ArtifactKind,
    fs: &dyn Filesystem,
) -> Result<String> {
    match kind {
        ArtifactKind::Agent => checksum_file_with(path, fs),
        ArtifactKind::Skill => checksum_dir_with(path, fs),
    }
}

/// Encode a byte slice as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Recursively collect all non-hidden files under `dir` via the given filesystem.
fn collect_files_with(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    let entries = fs
        .read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?;

    for entry in entries {
        if entry.file_name.starts_with('.') {
            continue;
        }
        if entry.is_dir {
            files.extend(collect_files_with(&entry.path, fs)?);
        } else {
            files.push(entry.path);
        }
    }
    Ok(files)
}
