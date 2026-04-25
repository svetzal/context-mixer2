use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::fs_util;
use crate::gateway::filesystem::Filesystem;
use crate::types::{ArtifactKind, LockEntry};

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
    let mut files = fs_util::collect_files_recursive(path, fs)?;
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

/// Returns `true` if the artifact on disk differs from the checksum recorded in
/// the lock entry when it was installed.
pub fn is_locally_modified(
    path: &Path,
    kind: ArtifactKind,
    lock_entry: &LockEntry,
    fs: &dyn Filesystem,
) -> Result<bool> {
    let current = checksum_artifact_with(path, kind, fs)?;
    Ok(current != lock_entry.installed_checksum)
}

/// Compute the current on-disk checksum and compare it to the lock entry.
/// Returns `(modified, Some(current_checksum))` when the file has been locally
/// modified, or `(false, None)` when it matches the recorded checksum.
pub fn current_checksum_if_modified(
    path: &Path,
    kind: ArtifactKind,
    lock_entry: &LockEntry,
    fs: &dyn Filesystem,
) -> Result<(bool, Option<String>)> {
    let current = checksum_artifact_with(path, kind, fs)?;
    if current == lock_entry.installed_checksum {
        Ok((false, None))
    } else {
        Ok((true, Some(current)))
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{make_lock_entry_with_checksum, test_paths};
    use crate::types::ArtifactKind;

    fn agent_path(paths: &crate::paths::ConfigPaths) -> std::path::PathBuf {
        ArtifactKind::Agent
            .installed_path("my-agent", &paths.install_dir(ArtifactKind::Agent, false))
    }

    #[test]
    fn is_locally_modified_returns_false_when_checksum_matches() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let content = "---\nname: my-agent\ndescription: test\n---\n";
        let path = agent_path(&paths);
        fs.add_file(path.clone(), content);

        // Compute the real checksum
        let cs = checksum_file_with(&path, &fs).unwrap();
        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "source",
            "my-agent.md",
            &cs,
        );
        // Override installed_checksum to match current disk content
        entry.installed_checksum = cs;

        let modified = is_locally_modified(&path, ArtifactKind::Agent, &entry, &fs).unwrap();
        assert!(!modified, "expected not modified when checksum matches");
    }

    #[test]
    fn is_locally_modified_returns_true_when_checksum_differs() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let path = agent_path(&paths);
        fs.add_file(path.clone(), "current content on disk");

        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "source",
            "my-agent.md",
            "sha256:placeholder",
        );
        entry.installed_checksum = "sha256:different_from_disk".to_string();

        let modified = is_locally_modified(&path, ArtifactKind::Agent, &entry, &fs).unwrap();
        assert!(modified, "expected modified when checksum differs");
    }

    #[test]
    fn current_checksum_if_modified_returns_none_when_unchanged() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let content = "---\nname: my-agent\ndescription: test\n---\n";
        let path = agent_path(&paths);
        fs.add_file(path.clone(), content);

        let cs = checksum_file_with(&path, &fs).unwrap();
        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "source",
            "my-agent.md",
            &cs,
        );
        entry.installed_checksum = cs;

        let (modified, disk_cs) =
            current_checksum_if_modified(&path, ArtifactKind::Agent, &entry, &fs).unwrap();
        assert!(!modified, "expected not modified");
        assert!(disk_cs.is_none(), "expected no disk checksum when unchanged");
    }

    #[test]
    fn current_checksum_if_modified_returns_some_when_changed() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let path = agent_path(&paths);
        fs.add_file(path.clone(), "modified content");

        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "source",
            "my-agent.md",
            "sha256:placeholder",
        );
        entry.installed_checksum = "sha256:original_recorded_checksum".to_string();

        let (modified, disk_cs) =
            current_checksum_if_modified(&path, ArtifactKind::Agent, &entry, &fs).unwrap();
        assert!(modified, "expected modified");
        assert!(disk_cs.is_some(), "expected disk checksum to be present");
        assert!(
            disk_cs.unwrap().starts_with("sha256:"),
            "disk checksum should be a sha256 value"
        );
    }
}
