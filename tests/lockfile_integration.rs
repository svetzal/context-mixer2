use cmx::gateway::real::RealFilesystem;
use cmx::lockfile::{load_from_with, save_to_with};
use cmx::types::{ArtifactKind, LockEntry, LockFile, LockSource};
use std::collections::BTreeMap;
use tempfile::TempDir;

fn sample_lock_file() -> LockFile {
    let mut packages = BTreeMap::new();
    packages.insert(
        "rust-craftsperson".to_string(),
        LockEntry {
            artifact_type: ArtifactKind::Agent,
            version: Some("1.0.0".to_string()),
            installed_at: "2024-01-01T00:00:00Z".to_string(),
            source: LockSource {
                repo: "guidelines".to_string(),
                path: "agents/rust-craftsperson.md".to_string(),
            },
            source_checksum: "sha256:abc123".to_string(),
            installed_checksum: "sha256:def456".to_string(),
        },
    );
    LockFile {
        version: 1,
        packages,
    }
}

#[test]
fn save_and_load_round_trips_all_fields() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cmx-lock.json");

    let original = sample_lock_file();
    save_to_with(&original, &path, &RealFilesystem).unwrap();
    let restored = load_from_with(&path, &RealFilesystem).unwrap();

    assert_eq!(restored.version, original.version);
    let entry = restored.packages.get("rust-craftsperson").expect("entry present");
    assert_eq!(entry.version.as_deref(), Some("1.0.0"));
    assert_eq!(entry.source.repo, "guidelines");
    assert_eq!(entry.source.path, "agents/rust-craftsperson.md");
    assert_eq!(entry.source_checksum, "sha256:abc123");
    assert_eq!(entry.installed_checksum, "sha256:def456");
    assert_eq!(entry.artifact_type, ArtifactKind::Agent);
}

#[test]
fn save_creates_parent_directories() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("dir").join("cmx-lock.json");

    let lock = LockFile {
        version: 1,
        packages: BTreeMap::new(),
    };
    save_to_with(&lock, &path, &RealFilesystem).unwrap();
    assert!(path.exists());
}

#[test]
fn load_nonexistent_path_returns_empty_default() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("does-not-exist.json");

    let lock = load_from_with(&path, &RealFilesystem).unwrap();
    assert_eq!(lock.version, 1);
    assert!(lock.packages.is_empty());
}

#[test]
fn load_invalid_json_returns_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, b"not valid json").unwrap();

    let result = load_from_with(&path, &RealFilesystem);
    assert!(result.is_err(), "expected Err for invalid JSON");
}

#[test]
fn load_empty_packages_produces_empty_map() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cmx-lock.json");

    let empty = LockFile {
        version: 1,
        packages: BTreeMap::new(),
    };
    save_to_with(&empty, &path, &RealFilesystem).unwrap();
    let restored = load_from_with(&path, &RealFilesystem).unwrap();
    assert!(restored.packages.is_empty());
}
