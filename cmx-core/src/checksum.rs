use crate::error::Result;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::fs_util;
use crate::gateway::filesystem::Filesystem;
use crate::types::{ArtifactKind, LockEntry};

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem)
// ---------------------------------------------------------------------------

/// Compute SHA-256 checksum for an agent (single .md file) via the given filesystem.
pub fn checksum_file(path: &Path, fs: &dyn Filesystem) -> Result<String> {
    let content = fs.read(path)?;
    let hash = Sha256::digest(&content);
    Ok(format!("sha256:{}", hex_encode(&hash)))
}

/// Compute SHA-256 checksum for a skill (directory) via the given filesystem.
/// Hashes all files in sorted order for determinism.
pub fn checksum_dir(path: &Path, fs: &dyn Filesystem) -> Result<String> {
    let files = fs_util::collect_files_recursive(path, fs)?;

    let mut entries: Vec<(std::path::PathBuf, Vec<u8>)> = files
        .iter()
        .map(|file| {
            let relative = file.strip_prefix(path).unwrap_or(file).to_path_buf();
            let content = fs.read(file)?;
            Ok((relative, content))
        })
        .collect::<Result<_>>()?;

    // Order by the `/`-joined relative-path string (SPEC §5.1 / §11.4), *not*
    // component-wise `Path` order, which diverges at the `.`-vs-`/` boundary
    // (e.g. `a.b` vs `a/b`). Keying on the same string as the in-memory bundle
    // keeps `checksum_dir` and `checksum_bundled` in agreement.
    entries.sort_by_key(|(rel, _)| rel_path_key(rel));

    Ok(checksum_in_memory(
        entries.iter().map(|(rel, content)| (rel.as_path(), content.as_slice())),
    ))
}

/// Compute a SHA-256 checksum over an in-memory set of `(relative_path, bytes)` pairs.
///
/// Each entry contributes its relative path (as UTF-8 bytes) then its content to
/// the hash stream. The result is prefixed with `sha256:`.
///
/// The caller is responsible for providing entries in a stable (sorted) order.
pub fn checksum_in_memory<'a>(
    entries: impl IntoIterator<Item = (&'a std::path::Path, &'a [u8])>,
) -> String {
    let mut hasher = Sha256::new();
    for (rel, content) in entries {
        hasher.update(rel_path_key(rel).as_bytes());
        hasher.update(content);
    }
    let hash = hasher.finalize();
    format!("sha256:{}", hex_encode(&hash))
}

/// The canonical hash/sort key for a relative artifact path: its components
/// joined with `/`, independent of the OS path separator (SPEC §5.1).
///
/// Both the checksum stream and the file ordering key on this, so a skill's
/// in-memory checksum and its on-disk checksum agree, and ordering stays stable
/// across platforms. On macOS/Linux this equals the plain path string for normal
/// relative paths, so it does not change existing checksums; it fixes the
/// `.`-vs-`/` ordering divergence (SPEC §11.4) and normalizes `\`-separated
/// paths a Windows port might produce (SPEC §11.3).
pub(crate) fn rel_path_key(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Compute the checksum for an artifact, dispatching to the correct strategy
/// based on its kind: file checksum for agents, directory checksum for skills.
pub fn checksum_artifact(path: &Path, kind: ArtifactKind, fs: &dyn Filesystem) -> Result<String> {
    match kind {
        ArtifactKind::Agent => checksum_file(path, fs),
        ArtifactKind::Skill => checksum_dir(path, fs),
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
    let current = checksum_artifact(path, kind, fs)?;
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
    let current = checksum_artifact(path, kind, fs)?;
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
    use crate::types::{ArtifactKind, InstallScope};

    fn agent_path(paths: &crate::paths::ConfigPaths) -> std::path::PathBuf {
        ArtifactKind::Agent.installed_path(
            "my-agent",
            &paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            ArtifactKind::HOME_AGENT_EXT,
        )
    }

    #[test]
    fn is_locally_modified_returns_false_when_checksum_matches() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let content = "---\nname: my-agent\ndescription: test\n---\n";
        let path = agent_path(&paths);
        fs.add_file(path.clone(), content);

        // Compute the real checksum
        let cs = checksum_file(&path, &fs).unwrap();
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

        let cs = checksum_file(&path, &fs).unwrap();
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
