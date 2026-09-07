//! End-to-end test of the pinned revision: the fixture atlas becomes
//! a real git repository, the manifest pins its first commit, and a second
//! commit changes one record and one validator script at `HEAD`. cmv must run
//! the *pinned* script (whose verdict differs from HEAD's), report the
//! atlas as moved, mark the changed intent stale, and flip to HEAD's
//! verdict under `--at-head`. Skips with a message when `git` is not on PATH.
//!
//! `HOME` points at the temp dir so the developer's own cmx source registry
//! does not resolve the manifest's `guidelines` source; the recorded name then
//! falls through to the recorded path with a warning, which is asserted too.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

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

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=cmv test",
            "-c",
            "user.email=cmv@example.com",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8").trim().to_string()
}

fn commit_all(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

const PUT_GATEWAYS: &str = "craftsperson/rust/put-gateways-at-effect-boundaries";

/// An atlas repository with two commits and a workspace whose
/// manifest pins the first.
struct Scenario {
    tmp: TempDir,
    kb: PathBuf,
    workspace: PathBuf,
    pinned: String,
    head: String,
}

impl Scenario {
    fn build() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let kb = tmp.path().join("kb");
        copy_tree(&fixtures().join("kb"), &kb);
        git(&kb, &["init", "-q"]);
        let pinned = commit_all(&kb, "atlas as compiled");

        let workspace = tmp.path().join("workspace");
        copy_tree(&fixtures().join("workspace"), &workspace);
        fs::create_dir_all(tmp.path().join("home")).expect("home dir");
        let mut scenario = Self {
            tmp,
            kb,
            workspace,
            pinned,
            head: String::new(),
        };
        scenario.write_manifest(&scenario.pinned.clone());

        // HEAD moves on: the record grows a line and its validator now fails.
        let record = scenario.kb.join("intents").join(format!("{PUT_GATEWAYS}.toml"));
        let mut body = fs::read_to_string(&record).unwrap();
        body.push_str("# corrected wording at HEAD\n");
        fs::write(&record, body).unwrap();
        let script = scenario.kb.join("checks/rust/put_gateways_at_effect_boundaries.sh");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' '{\"applicable\": true, \"followed\": false, \"evidence\": [\"HEAD script says no\"]}'\n",
        )
        .unwrap();
        scenario.head = commit_all(&scenario.kb, "tighten the gateway check");
        assert_ne!(scenario.head, scenario.pinned);
        scenario
    }

    fn write_manifest(&self, revision: &str) {
        let path = self.workspace.join(".context-mixer/cmf-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest["atlas"]["path"] = serde_json::Value::String(self.kb.display().to_string());
        manifest["atlas"]["revision"] = serde_json::Value::String(revision.to_string());
        fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    }

    fn cmv(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cmv"))
            .args(args)
            .arg("--root")
            .arg(&self.workspace)
            .env("HOME", self.tmp.path().join("home"))
            .output()
            .expect("cmv runs")
    }

    fn check_json(&self, extra: &[&str]) -> (Output, serde_json::Value) {
        let mut args = vec!["check", "--json"];
        args.extend_from_slice(extra);
        let output = self.cmv(&args);
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

fn intent<'a>(report: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    report["intents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|intent| intent["key"] == key)
        .unwrap_or_else(|| panic!("no intent {key} in {report}"))
}

#[test]
fn validators_run_from_the_pinned_tree_and_stale_compares_head() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let scenario = Scenario::build();
    let (output, report) = scenario.check_json(&[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "isolate-functional-core still fails: {stderr}");
    assert!(
        stderr.contains("cmx source \"guidelines\" is not registered"),
        "the recorded source falls through with a warning: {stderr}"
    );

    assert_eq!(
        report["atlas"],
        serde_json::json!({
            "path": scenario.kb.display().to_string(),
            "resolved_by": "path",
            "source": "guidelines",
            "pinned_revision": scenario.pinned,
            "head_revision": scenario.head,
            "verified_against": "pinned",
            "moved": true,
        })
    );

    let put_gateways = intent(&report, PUT_GATEWAYS);
    assert_eq!(put_gateways["state"], "pass", "the pinned script's verdict, not HEAD's");
    assert_eq!(put_gateways["stale"], true, "record and validator changed at HEAD");
    assert_eq!(
        intent(&report, "craftsperson/rust/compile-public-documentation")["stale"],
        false,
        "an untouched record is not stale just because HEAD moved"
    );

    let human = String::from_utf8(scenario.cmv(&["check"]).stdout).unwrap();
    let expected_line = format!(
        "atlas has moved: HEAD {} vs pinned {}; 2 compiled records or validators changed; re-run cmf install to recompile\n",
        &scenario.head[..12],
        &scenario.pinned[..12]
    );
    assert!(human.ends_with(&expected_line), "{human}");
}

#[test]
fn at_head_verifies_the_working_tree_and_flips_the_verdict() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let scenario = Scenario::build();
    let (_, report) = scenario.check_json(&["--at-head"]);
    assert_eq!(report["atlas"]["verified_against"], "head");
    assert_eq!(report["atlas"]["moved"], true);
    let put_gateways = intent(&report, PUT_GATEWAYS);
    assert_eq!(put_gateways["state"], "fail", "HEAD's script says no");
    assert_eq!(put_gateways["evidence"], serde_json::json!(["HEAD script says no"]));
    assert_eq!(put_gateways["stale"], true);
}

#[test]
fn status_reports_the_pin_and_the_move_without_running_validators() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let scenario = Scenario::build();
    let output = scenario.cmv(&["status"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(&format!("Pinned revision: {}\n", scenario.pinned)), "{text}");
    assert!(text.contains(&format!("HEAD revision: {}\n", scenario.head)), "{text}");
    assert!(text.contains("Verified against: pinned revision\n"), "{text}");
    assert!(text.contains("Stale records: 2\n"), "{text}");
    assert!(text.contains("atlas has moved: HEAD "), "{text}");
}

#[test]
fn head_equal_to_the_pin_uses_the_checkout_directly() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let scenario = Scenario::build();
    scenario.write_manifest(&scenario.head);
    let (_, report) = scenario.check_json(&[]);
    assert_eq!(report["atlas"]["moved"], false);
    assert_eq!(report["atlas"]["verified_against"], "pinned");
    assert_eq!(intent(&report, PUT_GATEWAYS)["state"], "fail", "HEAD is what is pinned now");
}

#[test]
fn unreachable_pinned_revision_exits_two_with_the_fetch_remedy() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let scenario = Scenario::build();
    let bogus = "0123456789abcdef0123456789abcdef01234567";
    scenario.write_manifest(bogus);
    let output = scenario.cmv(&["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(bogus), "{stderr}");
    assert!(stderr.contains("run `git fetch` there"), "{stderr}");
    assert!(stderr.contains("--at-head"), "{stderr}");

    let output = scenario.cmv(&["check", "--json", "--at-head"]);
    assert_eq!(output.status.code(), Some(1), "--at-head sidesteps the unreachable pin");
}
