//! Real-binary validation of the entire atlas, without invoking validators or writing state.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(path.strip_prefix(root).unwrap().to_owned(), fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

struct Case {
    tmp: TempDir,
    atlas: PathBuf,
    project: PathBuf,
    home: PathBuf,
}

impl Case {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let atlas = tmp.path().join("atlas");
        copy_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas"), &atlas);
        let project = tmp.path().join("project");
        let home = tmp.path().join("home");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&home).unwrap();
        Self {
            tmp,
            atlas,
            project,
            home,
        }
    }

    fn run(&self, json: bool) -> Output {
        let before = snapshot(self.tmp.path());
        let mut command = Command::new(env!("CARGO_BIN_EXE_cmf"));
        command
            .arg("--root")
            .arg(&self.atlas)
            .arg("validate")
            .current_dir(&self.project)
            .env("HOME", &self.home);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert_eq!(
            snapshot(self.tmp.path()),
            before,
            "validation wrote to atlas, project, or home"
        );
        output
    }
}

#[test]
fn valid_atlas_is_repeatable_and_never_runs_a_validator() {
    let case = Case::new();
    let validator = case.atlas.join("checks/rust/isolate_functional_core.sh");
    let marker = case.tmp.path().join("invoked");
    fs::write(&validator, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, case.run(true).stdout, "JSON changes between runs");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], true);
    assert_eq!(json["profiles"], 1);
    assert!(!marker.exists(), "validator must not execute");
    let human = case.run(false);
    assert_eq!(human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&human.stdout).contains("Atlas valid: 4 intents, 1 profiles"));
}

#[test]
fn invalid_validator_path_has_file_and_field_in_both_formats() {
    let case = Case::new();
    let validator = case.atlas.join("checks/rust/isolate_functional_core.sh");
    let mut permissions = fs::metadata(&validator).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&validator, permissions).unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, case.run(true).stdout);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], false);
    assert_eq!(json["diagnostics"][0]["field"], "evidence[1].run");
    assert!(
        json["diagnostics"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("isolate-functional-core.toml")
    );
    let human = String::from_utf8(case.run(false).stdout).unwrap();
    assert!(human.contains("[evidence[1].run]"));
}

#[test]
fn broken_profile_selection_and_sensor_are_reported() {
    let case = Case::new();
    let profile = case.atlas.join("profiles/rust-shipping.toml");
    let raw = fs::read_to_string(&profile).unwrap();
    fs::write(&profile, raw.replace("budget_tokens = 400", "budget_tokens = 1")).unwrap();
    fs::write(
        case.atlas.join("ecosystems.toml"),
        "[unknown]\nsignatures = [{ file = \"x\" }]\n",
    )
    .unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["file"].as_str().unwrap().ends_with("ecosystems.toml"))
    );
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["file"].as_str().unwrap().ends_with("rust-shipping.toml"))
    );
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["field"] == "budget_tokens")
    );
}

#[test]
fn malformed_record_names_the_record_and_evidence_field() {
    let case = Case::new();
    let record = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let raw = fs::read_to_string(&record).unwrap();
    fs::write(&record, raw.replace("run = \"checks/rust/isolate_functional_core.sh\", ", ""))
        .unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["file"], record.to_str().unwrap());
    assert_eq!(json["diagnostics"][0]["field"], "evidence[1].run");
}

#[test]
fn malformed_toml_keeps_catalog_path_and_names_missing_field() {
    let case = Case::new();
    let record = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let raw = fs::read_to_string(&record).unwrap();
    fs::write(&record, raw.replace("title = \"Isolate the functional core\"\n", "")).unwrap();
    let validator = case.atlas.join("checks/rust/isolate_functional_core.sh");
    let marker = case.tmp.path().join("validator-ran");
    fs::write(&validator, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();

    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, case.run(true).stdout);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["file"], record.to_str().unwrap());
    assert_eq!(json["diagnostics"][0]["field"], "title");
    assert!(!marker.exists());

    fs::write(&record, raw).unwrap();
    let corrected = case.run(true);
    assert_eq!(corrected.status.code(), Some(0));
    assert_eq!(corrected.stdout, case.run(true).stdout);
    assert!(!marker.exists());
}

#[test]
fn invalid_toml_syntax_keeps_catalog_path() {
    let case = Case::new();
    let record = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let raw = fs::read_to_string(&record).unwrap();
    fs::write(&record, raw.replace("title = \"Isolate the functional core\"", "title = ["))
        .unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["file"], record.to_str().unwrap());
    assert_eq!(json["diagnostics"][0]["field"], "record");
}

#[test]
fn invalid_validator_traversal_names_the_run_field() {
    let case = Case::new();
    let record = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let raw = fs::read_to_string(&record).unwrap();
    fs::write(&record, raw.replace("checks/rust/isolate_functional_core.sh", "../outside.sh"))
        .unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostics"][0]["file"], record.to_str().unwrap());
    assert_eq!(json["diagnostics"][0]["field"], "evidence[1].run");
}

#[test]
fn reports_every_broken_record_and_profile_in_path_order() {
    let case = Case::new();
    let first = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let second = case
        .atlas
        .join("intents/craftsperson/rust/put-gateways-at-effect-boundaries.toml");
    for (path, field) in [
        (&first, "run = \"checks/rust/isolate_functional_core.sh\", "),
        (&second, "language = \"rust\", "),
    ] {
        fs::write(path, fs::read_to_string(path).unwrap().replace(field, "")).unwrap();
    }
    let profile = case.atlas.join("profiles/rust-shipping.toml");
    fs::write(
        &profile,
        fs::read_to_string(&profile)
            .unwrap()
            .replace("budget_tokens = 400", "budget_tokens = 1"),
    )
    .unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics[0]["file"], first.to_str().unwrap());
    assert_eq!(diagnostics[0]["field"], "evidence[1].run");
    assert_eq!(diagnostics[1]["file"], second.to_str().unwrap());
    assert_eq!(diagnostics[1]["field"], "evidence[1].language");
    assert_eq!(diagnostics[2]["file"], profile.to_str().unwrap());
    assert_eq!(diagnostics[2]["field"], "select");
    assert_eq!(output.stdout, case.run(true).stdout);
}

#[test]
fn descriptive_static_check_does_not_need_a_validator_file() {
    let case = Case::new();
    let record = case.atlas.join("intents/craftsperson/rust/isolate-functional-core.toml");
    let raw = fs::read_to_string(&record).unwrap();
    let raw =
        raw.replace("language = \"rust\", run = \"checks/rust/isolate_functional_core.sh\", ", "");
    fs::write(&record, raw).unwrap();
    let output = case.run(true);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stdout));
}
