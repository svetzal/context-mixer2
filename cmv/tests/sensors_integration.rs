//! End-to-end tests of ecosystem sensing through the `cmv` binary: the
//! fixture atlas is copied to a temp dir with or without its `ecosystems.toml`,
//! and the fixture workspace is varied, so the three states `CMV.md`
//! documents each show up in a real run — an atlas with no sensors (every
//! validator-bearing intent unchecked, with the override hint), a `cmv.toml`
//! `ecosystems` override that restores verification without sensors, and a
//! workspace whose ecosystem is not the one the manifest's profile targeted.
//!
//! `HOME` points at the temp dir so the developer's own cmx source registry
//! never takes part.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const NO_SENSORS: &str = "atlas declares no sensors; set ecosystems in cmv.toml to override";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create target dir");
    for entry in fs::read_dir(from).expect("fixture dir readable") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

/// A temp copy of the fixture atlas and workspace, varied per test.
struct Scenario {
    tmp: TempDir,
    kb: PathBuf,
    workspace: PathBuf,
}

impl Scenario {
    fn build() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kb = tmp.path().join("kb");
        copy_tree(&fixtures().join("kb"), &kb);
        let workspace = tmp.path().join("workspace");
        copy_tree(&fixtures().join("workspace"), &workspace);
        Self { tmp, kb, workspace }
    }

    fn without_sensors(self) -> Self {
        fs::remove_file(self.kb.join("ecosystems.toml")).expect("fixture sensor file exists");
        self
    }

    fn with_sensors(self, raw: &str) -> Self {
        fs::write(self.kb.join("ecosystems.toml"), raw).expect("sensor file written");
        self
    }

    fn with_config(self, raw: &str) -> Self {
        fs::write(self.workspace.join("cmv.toml"), raw).expect("cmv.toml written");
        self
    }

    /// Replace the workspace's root files with `files`.
    fn with_root_files(self, files: &[&str]) -> Self {
        for entry in fs::read_dir(&self.workspace).expect("workspace readable") {
            let entry = entry.expect("entry");
            if entry.path().is_file() {
                fs::remove_file(entry.path()).expect("removed");
            }
        }
        for file in files {
            fs::write(self.workspace.join(file), "").expect("root file written");
        }
        self
    }

    fn with_profile_ecosystems(self, ecosystems: &[&str]) -> Self {
        let path = self.workspace.join(".context-mixer/cmf-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest["profile"]["ecosystems"] = serde_json::json!(ecosystems);
        fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        self
    }

    fn cmv(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cmv"))
            .args(args)
            .arg("--root")
            .arg(&self.workspace)
            .arg("--atlas")
            .arg(&self.kb)
            .env("HOME", self.tmp.path())
            .output()
            .expect("cmv runs")
    }

    fn check_json(&self) -> (Output, serde_json::Value) {
        let output = self.cmv(&["check", "--json"]);
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "not JSON ({error}): stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, report)
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf-8 stdout")
}

fn states(report: &serde_json::Value) -> Vec<(String, String, String)> {
    report["intents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|intent| {
            (
                intent["key"].as_str().unwrap().to_string(),
                intent["state"].as_str().unwrap().to_string(),
                intent["reason"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[test]
fn an_atlas_without_sensors_leaves_every_validator_bearing_intent_unchecked() {
    let scenario = Scenario::build().without_sensors();
    let (output, report) = scenario.check_json();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(report["atlas"]["sensors"], false);
    assert_eq!(report["ecosystems"], serde_json::json!([]));
    assert_eq!(report["profile_mismatch"], serde_json::json!([]));
    let states = states(&report);
    assert_eq!(
        states[0],
        (
            "craftsperson/rust/put-gateways-at-effect-boundaries".to_string(),
            "unchecked".to_string(),
            NO_SENSORS.to_string()
        )
    );
    assert_eq!(states[1].1, "unchecked");
    assert_eq!(states[1].2, NO_SENSORS);
    assert_eq!(
        states[2].2, "no validator for ecosystems []",
        "a record with no validator is unchecked for that reason, sensors or not"
    );
    assert_eq!(states[3].2, "record not in atlas");
    assert_eq!(report["summary"]["unchecked"], 4);

    let text = stdout(&scenario.cmv(&["check"]));
    assert!(text.contains(&format!("           {NO_SENSORS}\n")), "{text}");
    assert!(!text.contains("manifest profile targets"), "{text}");

    let summary = stdout(&scenario.cmv(&["status"]));
    assert!(summary.contains(&format!("Ecosystems: none ({NO_SENSORS})\n")), "{summary}");
    assert!(
        summary.contains(
            "Validators: 0 of 4 compiled intents have a validator for the workspace's ecosystems\n"
        ),
        "{summary}"
    );

    let explain = stdout(&scenario.cmv(&["explain", "craftsperson/rust/isolate-functional-core"]));
    assert!(explain.contains(&format!("Ecosystems: none ({NO_SENSORS})\n")), "{explain}");
    assert!(
        explain.contains(&format!(
            "  rust  checks/rust/isolate_functional_core.sh  (required)  skipped: {NO_SENSORS}\n"
        )),
        "{explain}"
    );
}

#[test]
fn a_cmv_toml_override_verifies_without_sensors_and_shows_where_it_came_from() {
    let scenario = Scenario::build().without_sensors().with_config(
        "ecosystems = [\"rust\"]\nvalidator_timeout_seconds = 10\n\n[intent.\"craftsperson/rust/isolate-functional-core\"]\nbusiness_rule_pattern = \"\\\\b500\\\\b\"\nbusiness_rule_minimum_matches = 2\n",
    );
    let (output, report) = scenario.check_json();
    assert_eq!(output.status.code(), Some(1), "the required validator runs and fails");
    assert_eq!(report["atlas"]["sensors"], false);
    assert_eq!(report["ecosystems"], serde_json::json!(["rust"]));
    let states = states(&report);
    assert_eq!(states[0].1, "pass");
    assert_eq!(states[1].1, "fail");
    assert_eq!(states[2].2, "no validator for ecosystems [rust]");

    let summary = stdout(&scenario.cmv(&["status"]));
    assert!(summary.contains("Ecosystems: rust (cmv.toml override)\n"), "{summary}");
}

#[test]
fn a_workspace_in_another_ecosystem_gets_the_profile_mismatch_line() {
    let scenario = Scenario::build()
        .with_sensors(
            "[rust]\nsignatures = [{ file = \"Cargo.toml\" }]\n\n[python]\nsignatures = [{ file = \"pyproject.toml\" }]\n",
        )
        .with_root_files(&["pyproject.toml"])
        .with_profile_ecosystems(&["rust"]);
    // The fixture atlas has no python records, so `validate` would reject a
    // `python` sensor; give it one.
    let python = scenario.kb.join("intents/craftsperson/python");
    fs::create_dir_all(&python).unwrap();
    fs::copy(
        scenario.kb.join("intents/craftsperson/rust/compile-public-documentation.toml"),
        python.join("compile-public-documentation.toml"),
    )
    .unwrap();

    let (output, report) = scenario.check_json();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(report["atlas"]["sensors"], true);
    assert_eq!(report["ecosystems"], serde_json::json!(["python"]));
    assert_eq!(report["profile_mismatch"], serde_json::json!(["rust"]));
    let states = states(&report);
    assert!(
        states.iter().take(4).all(|(_, state, _)| state == "unchecked"),
        "no rust validator applies to a python workspace: {states:?}"
    );
    assert_eq!(states[0].2, "no validator for ecosystems [python]");

    let text = stdout(&scenario.cmv(&["check"]));
    assert!(
        text.ends_with(
            "manifest profile targets rust but the workspace shows python; the guidance may be for a different ecosystem\n"
        ),
        "{text}"
    );
}

#[test]
fn an_invalid_sensor_file_is_an_atlas_error() {
    let scenario = Scenario::build().with_sensors("[go]\nsignatures = [{ file = \"go.mod\" }]\n");
    let output = scenario.cmv(&["check"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid sensors"), "{stderr}");
    assert!(
        stderr.contains("sensor ecosystem `go` is not a directory any record uses"),
        "{stderr}"
    );
}
