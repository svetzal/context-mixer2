//! Real-binary verification of two independently pinned local artifacts.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
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

struct Project {
    temp: TempDir,
    atlas: PathBuf,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let reference = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas");
        let atlas = temp.path().join("atlas");
        let root = temp.path().join("project");
        copy_tree(&reference, &atlas);
        copy_tree(&reference.join("cases/compliant"), &root);
        let profile = fs::read_to_string(atlas.join("profiles/rust-shipping.toml")).unwrap();
        fs::write(
            atlas.join("profiles/rust-skill.toml"),
            profile
                .replace("id = \"rust-shipping\"", "id = \"rust-skill\"")
                .replace("name = \"AGENTS\"", "name = \"rust-skill\"")
                .replace("surface = \"agent\"", "surface = \"skill\""),
        )
        .unwrap();
        let project = Self { temp, atlas, root };
        project.git(&["init", "-q"]);
        project.git(&["add", "."]);
        project.git(&["commit", "-qm", "Initial reference atlas"]);
        project
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.atlas)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn run(&self, binary: &str, args: &[&str]) -> Output {
        let path = Path::new(env!("CARGO_BIN_EXE_cmv")).with_file_name(binary);
        Command::new(path)
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("home/.config"))
            .env("XDG_CACHE_HOME", self.temp.path().join("home/.cache"))
            .output()
            .unwrap()
    }

    fn install(&self, profile: &str) {
        let output = self.run(
            "cmf",
            &[
                "--root",
                self.atlas.to_str().unwrap(),
                "install",
                profile,
                "--local",
                "--platform",
                "codex",
                "--apply",
                "--force",
            ],
        );
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    }

    fn manifest(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.root.join(".context-mixer/cmf-manifest.json")).unwrap(),
        )
        .unwrap()
    }

    fn check(&self, expected_exit: i32) -> Value {
        let output = self.run("cmv", &["check", "--json", "--atlas", self.atlas.to_str().unwrap()]);
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn check_verifies_both_artifacts_at_independent_revisions_and_reports_each_failure() {
    let project = Project::new();
    let first = project.git(&["rev-parse", "HEAD"]);
    project.install("rust-shipping");
    project.git(&["commit", "-qm", "Next atlas revision", "--allow-empty"]);
    let second = project.git(&["rev-parse", "HEAD"]);
    project.install("rust-skill");

    let manifest = project.manifest();
    assert_eq!(manifest["schema"], 2);
    let entries = manifest["artifacts"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["atlas"]["revision"], first);
    assert_eq!(entries[1]["atlas"]["revision"], second);
    assert_eq!(entries[0]["artifact"]["surface"], "agent");
    assert_eq!(entries[1]["artifact"]["surface"], "skill");
    assert_eq!(entries[0]["intents"].as_array().unwrap().len(), 3);
    assert_eq!(entries[1]["intents"].as_array().unwrap().len(), 3);

    let passing = project.check(0);
    assert_eq!(passing["artifacts"].as_array().unwrap().len(), 2);
    for artifact in passing["artifacts"].as_array().unwrap() {
        assert_eq!(artifact["check"]["summary"]["fail"], 0);
    }
    fs::copy(
        project.atlas.join("cases/violation/src/core.rs"),
        project.root.join("src/core.rs"),
    )
    .unwrap();
    let failing = project.check(1);
    assert_eq!(failing["summary"]["exit_code"], 1);
    for artifact in failing["artifacts"].as_array().unwrap() {
        let intents = artifact["check"]["intents"].as_array().unwrap();
        assert!(
            intents
                .iter()
                .any(|intent| intent["key"] == "craftsperson/rust/isolate-functional-core"
                    && intent["state"] == "fail")
        );
    }
}

#[test]
fn updating_one_profile_preserves_the_other_compilation_record() {
    let project = Project::new();
    project.install("rust-shipping");
    project.install("rust-skill");
    let before = project.manifest();
    let skill = before["artifacts"][1].clone();
    let profile_path = project.atlas.join("profiles/rust-shipping.toml");
    let profile = fs::read_to_string(&profile_path).unwrap();
    fs::write(&profile_path, profile.replace("version = \"0.3.0\"", "version = \"0.3.1\""))
        .unwrap();
    project.git(&["add", "."]);
    project.git(&["commit", "-qm", "Update agent profile"]);
    project.install("rust-shipping");
    let after = project.manifest();
    assert_eq!(after["artifacts"][1], skill);
    assert_eq!(after["artifacts"][0]["profile"]["version"], "0.3.1");
    assert_eq!(project.check(0)["artifacts"].as_array().unwrap().len(), 2);

    let agent = after["artifacts"][0].clone();
    let skill_path = project.atlas.join("profiles/rust-skill.toml");
    let skill_profile = fs::read_to_string(&skill_path).unwrap();
    fs::write(&skill_path, skill_profile.replace("version = \"0.3.0\"", "version = \"0.3.2\""))
        .unwrap();
    project.git(&["add", "."]);
    project.git(&["commit", "-qm", "Update skill profile"]);
    project.install("rust-skill");
    let final_manifest = project.manifest();
    assert_eq!(final_manifest["artifacts"][0], agent);
    assert_eq!(final_manifest["artifacts"][1]["profile"]["version"], "0.3.2");
    assert_eq!(project.check(0)["artifacts"].as_array().unwrap().len(), 2);
}
