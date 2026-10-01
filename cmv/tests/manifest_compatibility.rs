//! Real-binary schema refusal before atlas lookup or validator execution.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

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

fn run(project: &Path, manifest: &Path, verb: &str, atlas: &Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cmv"));
    command.arg(verb);
    if verb == "explain" {
        command.arg("craftsperson/rust/isolate-functional-core");
    }
    command
        .args([
            "--root",
            project.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
            "--atlas",
            atlas.to_str().unwrap(),
        ])
        .current_dir(project)
        .env("HOME", project)
        .env("XDG_CONFIG_HOME", project.join("config"))
        .output()
        .unwrap()
}

#[test]
fn unsupported_schema_refuses_all_commands_before_atlas_resolution() {
    let temp = tempdir().unwrap();
    let project = temp.path();
    let manifest = project.join("manifest.json");
    // A later schema can change the rest of the document completely.
    fs::write(&manifest, br#"{"schema":2,"future_field":true}"#).unwrap();
    let absent_atlas = project.join("absent-atlas");
    for verb in ["check", "status", "explain"] {
        let output = run(project, &manifest, verb, &absent_atlas);
        assert_eq!(output.status.code(), Some(2), "{verb}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("unsupported manifest schema 2"), "{stderr}");
        assert!(stderr.contains(manifest.to_str().unwrap()));
        assert!(!stderr.contains("atlas at"), "{stderr}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn schema_one_knowledge_base_manifest_still_loads() {
    let temp = tempdir().unwrap();
    let project = temp.path();
    let manifest = project.join("manifest.json");
    let raw =
        include_str!("../../reference-atlas/regressions/compile-manifest/expected-manifest.json");
    let mut value: Value = serde_json::from_str(raw).unwrap();
    value["knowledge_base"] = value["atlas"].take();
    value.as_object_mut().unwrap().remove("atlas");
    value["knowledge_base"]["revision"] = Value::Null;
    value["knowledge_base"]["source"] = Value::Null;
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let atlas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas");
    let output = run(project, &manifest, "status", &atlas);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn schema_refusal_does_not_run_a_validator_or_write_project_files() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let atlas = temp.path().join("atlas");
    let reference = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas");
    copy_tree(&reference, &atlas);
    copy_tree(&reference.join("cases/compliant"), &project);
    let marker = project.join("validator-ran");
    let validator = atlas.join("checks/rust/isolate_functional_core.sh");
    fs::write(&validator, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    let mut permissions = fs::metadata(&validator).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&validator, permissions).unwrap();
    let manifest = project.join("manifest.json");
    let mut value: Value = serde_json::from_str(include_str!(
        "../../reference-atlas/regressions/compile-manifest/expected-manifest.json"
    ))
    .unwrap();
    value["atlas"]["source"] = Value::Null;
    value["atlas"]["revision"] = Value::Null;
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let baseline = run(&project, &manifest, "check", &atlas);
    assert_ne!(baseline.status.code(), Some(2), "{}", String::from_utf8_lossy(&baseline.stderr));
    assert!(marker.exists(), "schema 1 should reach the sentinel validator");
    fs::remove_file(&marker).unwrap();
    value["schema"] = 99.into();
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = fs::read(&manifest).unwrap();
    for verb in ["check", "status", "explain"] {
        let output = run(&project, &manifest, verb, &atlas);
        assert_eq!(output.status.code(), Some(2), "{verb}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unsupported manifest schema 99"),
            "{verb}"
        );
        assert!(!marker.exists(), "{verb} ran a validator");
        assert_eq!(fs::read(&manifest).unwrap(), before, "{verb} wrote manifest");
        assert!(!project.join(".context-mixer").exists(), "{verb} wrote state");
    }
}
