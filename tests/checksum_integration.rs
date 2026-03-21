use cmx::checksum::{checksum_artifact, checksum_dir, checksum_file};
use cmx::types::ArtifactKind;
use std::fs;
use tempfile::TempDir;

// --- File checksum ---

#[test]
fn checksum_file_produces_sha256_prefix() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("file.md");
    fs::write(&path, b"hello world").unwrap();
    let cs = checksum_file(&path).unwrap();
    assert!(cs.starts_with("sha256:"), "expected sha256: prefix, got: {cs}");
}

#[test]
fn checksum_file_is_deterministic() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("file.md");
    fs::write(&path, b"deterministic content").unwrap();
    let cs1 = checksum_file(&path).unwrap();
    let cs2 = checksum_file(&path).unwrap();
    assert_eq!(cs1, cs2);
}

#[test]
fn checksum_file_differs_for_different_content() {
    let dir = TempDir::new().unwrap();
    let path_a = dir.path().join("a.md");
    let path_b = dir.path().join("b.md");
    fs::write(&path_a, b"content A").unwrap();
    fs::write(&path_b, b"content B").unwrap();
    let cs_a = checksum_file(&path_a).unwrap();
    let cs_b = checksum_file(&path_b).unwrap();
    assert_ne!(cs_a, cs_b);
}

#[test]
fn checksum_file_same_content_same_checksum() {
    let dir = TempDir::new().unwrap();
    let path_a = dir.path().join("a.md");
    let path_b = dir.path().join("b.md");
    let content = b"identical content";
    fs::write(&path_a, content).unwrap();
    fs::write(&path_b, content).unwrap();
    let cs_a = checksum_file(&path_a).unwrap();
    let cs_b = checksum_file(&path_b).unwrap();
    assert_eq!(cs_a, cs_b);
}

// --- Directory checksum ---

#[test]
fn checksum_dir_is_deterministic() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("SKILL.md"), b"# Skill\n").unwrap();
    fs::write(dir.path().join("prompt.md"), b"# Prompt\n").unwrap();

    let cs1 = checksum_dir(dir.path()).unwrap();
    let cs2 = checksum_dir(dir.path()).unwrap();
    assert_eq!(cs1, cs2);
}

#[test]
fn checksum_dir_changes_when_file_content_changes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("SKILL.md");
    fs::write(&file, b"# Original\n").unwrap();

    let cs_before = checksum_dir(dir.path()).unwrap();
    fs::write(&file, b"# Modified\n").unwrap();
    let cs_after = checksum_dir(dir.path()).unwrap();

    assert_ne!(cs_before, cs_after);
}

#[test]
fn checksum_dir_excludes_dotfiles() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("SKILL.md"), b"# Skill\n").unwrap();

    let cs_without = checksum_dir(dir.path()).unwrap();

    // Add a dotfile — checksum must not change
    fs::write(dir.path().join(".DS_Store"), b"mac metadata").unwrap();
    let cs_with = checksum_dir(dir.path()).unwrap();

    assert_eq!(cs_without, cs_with, "dotfiles must not affect the directory checksum");
}

#[test]
fn checksum_dir_changes_when_new_file_added() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("SKILL.md"), b"# Skill\n").unwrap();

    let cs_before = checksum_dir(dir.path()).unwrap();
    fs::write(dir.path().join("extra.md"), b"# Extra\n").unwrap();
    let cs_after = checksum_dir(dir.path()).unwrap();

    assert_ne!(cs_before, cs_after);
}

// --- checksum_artifact dispatch ---

#[test]
fn checksum_artifact_agent_dispatches_to_checksum_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("my-agent.md");
    fs::write(&path, b"# Agent\n").unwrap();

    let via_artifact = checksum_artifact(&path, ArtifactKind::Agent).unwrap();
    let via_file = checksum_file(&path).unwrap();

    assert_eq!(via_artifact, via_file, "checksum_artifact(Agent) must match checksum_file");
}

#[test]
fn checksum_artifact_skill_dispatches_to_checksum_dir() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("SKILL.md"), b"# Skill\n").unwrap();
    fs::write(dir.path().join("prompt.md"), b"# Prompt\n").unwrap();

    let via_artifact = checksum_artifact(dir.path(), ArtifactKind::Skill).unwrap();
    let via_dir = checksum_dir(dir.path()).unwrap();

    assert_eq!(via_artifact, via_dir, "checksum_artifact(Skill) must match checksum_dir");
}
