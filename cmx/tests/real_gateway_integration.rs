//! Integration tests exercising the real filesystem/clock/git gateways.

use cmx::gateway::clock::Clock;
use cmx::gateway::filesystem::Filesystem;
use cmx::gateway::git::GitClient;
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use std::fs;
use tempfile::TempDir;

fn fs() -> RealFilesystem {
    RealFilesystem
}

#[test]
fn write_then_read_to_string_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("hello.txt");
    fs().write(&path, "hello world").unwrap();
    let content = fs().read_to_string(&path).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn write_bytes_then_read_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bytes.bin");
    let data: Vec<u8> = vec![1, 2, 3, 255];
    fs().write_bytes(&path, &data).unwrap();
    let result = fs().read(&path).unwrap();
    assert_eq!(result, data);
}

#[test]
fn exists_false_for_missing_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent.txt");
    assert!(!fs().exists(&path));
}

#[test]
fn exists_true_after_write() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("present.txt");
    fs().write(&path, "content").unwrap();
    assert!(fs().exists(&path));
}

#[test]
fn is_file_true_for_file_false_for_dir() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("a.txt");
    fs().write(&file_path, "x").unwrap();
    assert!(fs().is_file(&file_path));
    assert!(!fs().is_file(dir.path()));
}

#[test]
fn is_dir_true_for_dir_false_for_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("a.txt");
    fs().write(&file_path, "x").unwrap();
    assert!(fs().is_dir(dir.path()));
    assert!(!fs().is_dir(&file_path));
}

#[test]
fn create_dir_all_creates_nested_directories() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("a").join("b").join("c");
    fs().create_dir_all(&nested).unwrap();
    assert!(nested.is_dir());
}

#[test]
fn copy_file_copies_content_accurately() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dest = dir.path().join("dest.txt");
    fs::write(&src, b"original content").unwrap();
    fs().copy_file(&src, &dest).unwrap();
    let copied = fs::read_to_string(&dest).unwrap();
    assert_eq!(copied, "original content");
}

#[test]
fn remove_file_removes_file_and_exists_returns_false() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("to_remove.txt");
    fs().write(&path, "data").unwrap();
    assert!(fs().exists(&path));
    fs().remove_file(&path).unwrap();
    assert!(!fs().exists(&path));
}

#[test]
fn remove_dir_all_removes_directory_tree() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("file.txt"), b"data").unwrap();
    assert!(sub.exists());
    fs().remove_dir_all(&sub).unwrap();
    assert!(!sub.exists());
}

#[test]
fn read_dir_lists_children_with_correct_metadata() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("alpha.md"), b"a").unwrap();
    fs::write(dir.path().join("beta.md"), b"b").unwrap();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();

    let mut entries = fs().read_dir(dir.path()).unwrap();
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    let names: Vec<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();
    assert!(names.contains(&"alpha.md"), "alpha.md missing from {names:?}");
    assert!(names.contains(&"beta.md"), "beta.md missing from {names:?}");
    assert!(names.contains(&"subdir"), "subdir missing from {names:?}");

    let subdir_entry = entries.iter().find(|e| e.file_name == "subdir").unwrap();
    assert!(subdir_entry.is_dir);

    let file_entry = entries.iter().find(|e| e.file_name == "alpha.md").unwrap();
    assert!(!file_entry.is_dir);
}

#[test]
fn canonicalize_resolves_real_path_without_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("file.txt");
    fs::write(&path, b"x").unwrap();
    let canonical = fs().canonicalize(&path).unwrap();
    assert!(canonical.is_absolute());
}

#[test]
fn read_to_string_on_missing_file_returns_error() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("does_not_exist.txt");
    let result = fs().read_to_string(&missing);
    assert!(result.is_err());
}

#[test]
fn rename_moves_file_old_path_gone_new_path_has_content() {
    let dir = TempDir::new().unwrap();
    let old_path = dir.path().join("original.txt");
    let new_path = dir.path().join("renamed.txt");
    fs().write(&old_path, "original content").unwrap();
    fs().rename(&old_path, &new_path).unwrap();
    assert!(!fs().exists(&old_path));
    assert!(fs().exists(&new_path));
    let content = fs().read_to_string(&new_path).unwrap();
    assert_eq!(content, "original content");
}

#[test]
fn system_clock_now_is_between_before_and_after() {
    let before = chrono::Utc::now();
    let t = SystemClock.now();
    let after = chrono::Utc::now();
    assert!(before <= t && t <= after);
}

#[test]
fn real_git_client_pull_on_non_git_directory_errors() {
    let dir = TempDir::new().unwrap();
    let result = RealGitClient.pull(dir.path());
    assert!(result.is_err());
}

#[test]
fn real_git_client_clone_from_local_bare_repo() {
    let src = TempDir::new().unwrap();
    // Init a real git repo with a commit so we can clone it.
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(src.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(src.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(src.path())
        .output()
        .unwrap();
    fs::write(src.path().join("README.md"), b"hello").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(src.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(src.path())
        .output()
        .unwrap();

    let dest = TempDir::new().unwrap();
    let dest_path = dest.path().join("clone");
    let src_url = src.path().to_str().unwrap().to_string();

    let result = RealGitClient.clone_repo(&src_url, &dest_path);
    assert!(result.is_ok(), "clone failed: {:?}", result.err());
    assert!(dest_path.exists());
}
