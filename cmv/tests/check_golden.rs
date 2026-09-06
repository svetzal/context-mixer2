//! End-to-end golden test: the `cmv` binary against a fixture knowledge base
//! whose validators are `/bin/sh` scripts, run through the real process
//! runner on a temporary copy of the fixture workspace.
//!
//! The fixture exercises one intent per interesting path: a passing required
//! validator (which also proves `--workspace` reached it), a failing required
//! validator on a stale record (which echoes its `--config` back as signals,
//! proving the `cmv.toml` block reached it), a record with no validator, a
//! manifest entry with no record, and a dropped intent. The JSON and the human
//! listing are pinned byte for byte.
//!
//! cmv runs with the fixtures directory as its working directory and
//! `--knowledge-base kb`, so the `knowledge_base.path` it reports is the same
//! relative path on every machine, and with `HOME` pointed at the temp dir so
//! the developer's own cmx source registry never takes part.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Copy the fixture workspace (including its dot-directory) into a temp dir.
fn workspace_copy() -> TempDir {
    let tmp = tempfile::tempdir().expect("temp dir");
    copy_tree(&fixtures().join("workspace"), tmp.path());
    tmp
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

fn cmv(args: &[&str], workspace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cmv"))
        .args(args)
        .arg("--root")
        .arg(workspace)
        .args(["--knowledge-base", "kb"])
        .current_dir(fixtures())
        .env("HOME", workspace)
        .output()
        .expect("cmv runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf-8 stdout")
}

fn assert_golden(actual: &str, golden: &str) {
    let path = fixtures().join(golden);
    let expected = fs::read_to_string(&path).expect("golden file exists");
    assert_eq!(
        actual,
        expected,
        "output drifted from {}; if the change is intended, update the golden file",
        path.display()
    );
}

#[test]
fn check_json_matches_golden_and_exits_one_on_the_required_failure() {
    let workspace = workspace_copy();
    let output = cmv(&["check", "--json"], workspace.path());
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden(&stdout(&output), "expected-check.json");
}

#[test]
fn check_human_listing_matches_golden() {
    let workspace = workspace_copy();
    let output = cmv(&["check"], workspace.path());
    assert_eq!(output.status.code(), Some(1));
    assert_golden(&stdout(&output), "expected-check.txt");
}

#[test]
fn check_is_byte_identical_across_runs() {
    let workspace = workspace_copy();
    let first = stdout(&cmv(&["check", "--json"], workspace.path()));
    let second = stdout(&cmv(&["check", "--json"], workspace.path()));
    assert_eq!(first, second);
}

#[test]
fn strict_check_still_exits_one_here_and_reports_the_unchecked_intents() {
    let workspace = workspace_copy();
    let output = cmv(&["check", "--strict", "--json"], workspace.path());
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(report["summary"]["unchecked"], 2);
    assert_eq!(report["summary"]["exit_code"], 1);
}

#[test]
fn status_reports_the_pin_languages_and_coverage_without_running_validators() {
    let workspace = workspace_copy();
    let output = cmv(&["status"], workspace.path());
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("Profile: rust-shipping 0.3.0\n"), "{text}");
    assert!(
        text.contains("Knowledge base: kb (present, resolved by --knowledge-base)\n"),
        "{text}"
    );
    assert!(text.contains("Source: guidelines\n"), "{text}");
    assert!(
        text.contains("Pinned revision: a1b2c3d4e5f60718293a4b5c6d7e8f9012345678\n"),
        "{text}"
    );
    assert!(text.contains("HEAD revision: unavailable\n"), "{text}");
    assert!(text.contains("Verified against: working tree (HEAD)\n"), "{text}");
    assert!(text.contains("Languages: rust\n"), "{text}");
    assert!(text.contains("Intents: 4 compiled, 1 dropped\n"), "{text}");
    assert!(
        text.contains(
            "Validators: 2 of 4 compiled intents have a validator for the detected languages\n"
        ),
        "{text}"
    );
    assert!(text.contains("Stale records: 1\n"), "{text}");
    assert!(text.contains("Missing records: 1\n"), "{text}");
}

#[test]
fn explain_names_the_validators_and_the_argv_without_running_anything() {
    let workspace = workspace_copy();
    let output = cmv(&["explain", "craftsperson/rust/isolate-functional-core"], workspace.path());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let text = stdout(&output);
    assert!(
        text.starts_with("Intent: craftsperson/rust/isolate-functional-core\n"),
        "{text}"
    );
    assert!(text.contains("Record: resolved by key\n"), "{text}");
    assert!(text.contains("Title: Isolate the functional core\n"), "{text}");
    assert!(text.contains("Stale: yes"), "{text}");
    assert!(
        text.contains(
            "Config: {\"business_rule_minimum_matches\":2,\"business_rule_pattern\":\"\\\\b500\\\\b\"}\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("  rust  checks/rust/isolate_functional_core.sh  (required)  would run\n"),
        "{text}"
    );
    assert!(
        text.contains("  python  checks/python/isolate_functional_core.py  (required)  skipped: language python is not among the workspace's [rust]\n"),
        "{text}"
    );
    let kb = fixtures().join("kb").canonicalize().unwrap();
    assert!(
        text.contains(&format!(
            "    {}/checks/rust/isolate_functional_core.sh --workspace {} --config <scratch>/1.json\n",
            kb.display(),
            workspace.path().display()
        )),
        "{text}"
    );
}

#[test]
fn explain_json_matches_the_record_id_form_and_exits_two_when_unknown() {
    let workspace = workspace_copy();
    let output = cmv(
        &[
            "explain",
            "fixture.intent.put-gateways-at-effect-boundaries",
            "--json",
        ],
        workspace.path(),
    );
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(report["intent"]["key"], "craftsperson/rust/put-gateways-at-effect-boundaries");
    assert_eq!(report["intent"]["resolution"], "key");
    assert_eq!(report["intent"]["stale"], false);
    assert_eq!(report["intent"]["validators"][0]["would_run"], true);
    assert_eq!(report["knowledge_base"]["resolved_by"], "override");

    let output = cmv(&["explain", "craftsperson/rust/not-compiled"], workspace.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not in the manifest"));
}

#[test]
fn missing_manifest_exits_two_with_the_cmf_remedy() {
    let workspace = tempfile::tempdir().expect("temp dir");
    let output = cmv(&["check"], workspace.path());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no compile manifest at"), "{stderr}");
    assert!(stderr.contains("cmf install --local --apply"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn unreadable_knowledge_base_exits_two() {
    let workspace = workspace_copy();
    let output = Command::new(env!("CARGO_BIN_EXE_cmv"))
        .args(["check", "--root"])
        .arg(workspace.path())
        .args(["--knowledge-base", "/nonexistent/kb"])
        .output()
        .expect("cmv runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("/nonexistent/kb"));
}
