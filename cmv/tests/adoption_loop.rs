//! Real cmf-to-cmv contract against the project-owned reference atlas.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const REQUIRED: &str = "craftsperson/rust/isolate-functional-core";
const GATEWAY: &str = "craftsperson/rust/put-gateways-at-effect-boundaries";
const UNCHECKED: &str = "craftsperson/rust/compile-public-documentation";

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create target");
    for entry in fs::read_dir(from).expect("read source") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn git(atlas: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(atlas)
        .env("GIT_AUTHOR_NAME", "Reference Atlas")
        .env("GIT_AUTHOR_EMAIL", "reference@example.invalid")
        .env("GIT_COMMITTER_NAME", "Reference Atlas")
        .env("GIT_COMMITTER_EMAIL", "reference@example.invalid")
        .output()
        .expect("git runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("git stdout")
}

fn binary(name: &str) -> PathBuf {
    let current = Path::new(env!("CARGO_BIN_EXE_cmv"));
    current.with_file_name(name)
}

fn run(binary: &Path, args: &[&str], root: &Path, home: &Path) -> Output {
    Command::new(binary)
        .args(args)
        .current_dir(root)
        .env("HOME", home)
        .output()
        .expect("binary runs")
}

fn report(output: &Output, expected_code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("report JSON")
}

fn state<'a>(report: &'a Value, key: &str) -> &'a str {
    report["intents"]
        .as_array()
        .expect("intents array")
        .iter()
        .find(|intent| intent["key"] == key)
        .and_then(|intent| intent["state"].as_str())
        .expect("intent state")
}

#[test]
fn codex_preview_apply_and_pinned_verification_cycle() {
    let tmp = TempDir::new().expect("temp root");
    let atlas = tmp.path().join("atlas");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas");
    copy_tree(&source, &atlas);
    git(&atlas, &["init", "-q"]);
    git(&atlas, &["add", "."]);
    git(&atlas, &["commit", "-qm", "Reference atlas"]);
    let pin = git(&atlas, &["rev-parse", "HEAD"]).trim().to_owned();

    let project = tmp.path().join("project");
    copy_tree(&atlas.join("cases/compliant"), &project);
    let home = tmp.path().join("home");
    fs::create_dir(&home).expect("isolated home");
    let guidance = b"# Hand-authored project guidance\n";
    fs::write(project.join("AGENTS.md"), guidance).expect("project guidance");
    let atlas_arg = atlas.to_str().expect("atlas path");
    let install = [
        "--root",
        atlas_arg,
        "install",
        "rust-shipping",
        "--local",
        "--platform",
        "codex",
    ];
    let preview = run(&binary("cmf"), &install, &project, &home);
    assert_eq!(preview.status.code(), Some(0), "{}", String::from_utf8_lossy(&preview.stderr));
    assert!(!project.join(".codex/agents/AGENTS.toml").exists());
    assert!(!project.join(".context-mixer").exists());
    assert!(!home.join(".config/context-mixer").exists());
    assert_eq!(fs::read(project.join("AGENTS.md")).unwrap(), guidance);

    let applied = run(&binary("cmf"), &[&install[..], &["--apply"]].concat(), &project, &home);
    assert_eq!(applied.status.code(), Some(0), "{}", String::from_utf8_lossy(&applied.stderr));
    let artifact = project.join(".codex/agents/AGENTS.toml");
    assert!(artifact.exists());
    assert!(project.join(".context-mixer/cmx-lock-codex.json").exists());
    assert_eq!(fs::read(project.join("AGENTS.md")).unwrap(), guidance);
    let manifest: Value = serde_json::from_slice(
        &fs::read(project.join(".context-mixer/cmf-manifest.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    assert_eq!(manifest["profile"]["id"], "rust-shipping");
    assert_eq!(manifest["artifact"]["surface"], "agent");
    assert_eq!(manifest["atlas"]["revision"], pin);
    assert_eq!(manifest["atlas"]["path"], atlas_arg);
    let keys: Vec<_> = manifest["intents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|intent| intent["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&REQUIRED));
    assert!(keys.contains(&GATEWAY));
    assert!(keys.contains(&UNCHECKED));
    assert_eq!(keys.len(), 3);
    assert!(manifest["intents"].as_array().unwrap().iter().any(|intent| {
        intent["key"] == REQUIRED && intent["id"] == "context-mixer.intent.isolate-functional-core"
    }));

    let check = |args: &[&str]| run(&binary("cmv"), args, &project, &home);
    let compliant = report(&check(&["check", "--json", "--atlas", atlas_arg]), 0);
    assert_eq!(state(&compliant, REQUIRED), "pass");
    assert_eq!(state(&compliant, GATEWAY), "pass");
    assert_eq!(state(&compliant, UNCHECKED), "unchecked");
    assert_eq!(compliant["atlas"]["pinned_revision"], pin);

    fs::copy(
        atlas.join("cases/violation/src/violation.marker"),
        project.join("src/violation.marker"),
    )
    .expect("introduce violation");
    let violating = report(&check(&["check", "--json", "--atlas", atlas_arg]), 1);
    assert_eq!(state(&violating, REQUIRED), "fail");
    fs::remove_file(project.join("src/violation.marker")).expect("correct violation");
    let corrected = report(&check(&["check", "--json", "--atlas", atlas_arg]), 0);
    assert_eq!(state(&corrected, REQUIRED), "pass");
    let strict = report(&check(&["check", "--strict", "--json", "--atlas", atlas_arg]), 1);
    assert_eq!(state(&strict, UNCHECKED), "unchecked");

    let validator = atlas.join("checks/rust/isolate_functional_core.sh");
    fs::write(
        &validator,
        "#!/bin/sh\nprintf '%s\\n' '{\"applicable\":true,\"followed\":false}'\n",
    )
    .expect("change HEAD validator");
    git(&atlas, &["add", "."]);
    git(&atlas, &["commit", "-qm", "Move atlas HEAD"]);
    let new_head = git(&atlas, &["rev-parse", "HEAD"]).trim().to_owned();
    assert_ne!(new_head, pin);
    let moved = report(&check(&["check", "--json", "--atlas", atlas_arg]), 0);
    assert_eq!(state(&moved, REQUIRED), "pass");
    assert_eq!(moved["atlas"]["pinned_revision"], pin);
    assert_eq!(moved["atlas"]["head_revision"], new_head);
    assert_eq!(moved["atlas"]["moved"], true);
}
