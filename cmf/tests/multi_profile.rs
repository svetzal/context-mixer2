//! Local manifest persistence across agent and skill installs.

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
        Self { temp, atlas, root }
    }

    fn run(&self, profile: &str, apply: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cmf"));
        command.args([
            "--root",
            self.atlas.to_str().unwrap(),
            "install",
            profile,
            "--local",
            "--platform",
            "codex",
        ]);
        if apply {
            command.arg("--apply");
        }
        command
            .current_dir(&self.root)
            .env("HOME", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("home/.config"))
            .output()
            .unwrap()
    }

    fn manifest_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(".context-mixer/cmf-manifest.json")).unwrap()
    }
}

#[test]
fn second_install_retains_agent_and_preview_does_not_persist() {
    let project = Project::new();
    assert!(project.run("rust-shipping", true).status.success());
    let agent_only = project.manifest_bytes();
    assert!(project.run("rust-skill", false).status.success());
    assert_eq!(project.manifest_bytes(), agent_only);
    assert!(project.run("rust-skill", true).status.success());
    let manifest: Value = serde_json::from_slice(&project.manifest_bytes()).unwrap();
    let entries = manifest["artifacts"].as_array().unwrap();
    assert_eq!(manifest["schema"], 2);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["artifact"]["name"], "AGENTS");
    assert_eq!(entries[1]["artifact"]["name"], "rust-skill");
    assert!(entries.iter().all(|entry| entry["intents"].as_array().unwrap().len() == 3));
}

#[test]
fn version_refusal_keeps_other_record_and_manifest_bytes() {
    let project = Project::new();
    assert!(project.run("rust-shipping", true).status.success());
    assert!(project.run("rust-skill", true).status.success());
    let before = project.manifest_bytes();
    let profile_path = project.atlas.join("profiles/rust-shipping.toml");
    let profile = fs::read_to_string(&profile_path).unwrap();
    fs::write(&profile_path, profile.replace("version = \"0.3.0\"", "version = \"0.2.0\""))
        .unwrap();
    let refusal = project.run("rust-shipping", true);
    assert!(!refusal.status.success(), "downgrade must refuse");
    assert_eq!(project.manifest_bytes(), before);
    assert!(project.root.join(".agents/skills/rust-skill/SKILL.md").exists());
}

#[test]
fn local_install_migrates_legacy_knowledge_base_record_without_losing_it() {
    let project = Project::new();
    assert!(project.run("rust-shipping", true).status.success());
    let path = project.root.join(".context-mixer/cmf-manifest.json");
    let current: Value = serde_json::from_slice(&project.manifest_bytes()).unwrap();
    let mut legacy = current["artifacts"][0].clone();
    legacy["knowledge_base"] = legacy["atlas"].take();
    legacy.as_object_mut().unwrap().remove("atlas");
    fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

    assert!(project.run("rust-skill", true).status.success());
    let migrated: Value = serde_json::from_slice(&project.manifest_bytes()).unwrap();
    assert_eq!(migrated["schema"], 2);
    assert_eq!(migrated["artifacts"].as_array().unwrap().len(), 2);
    assert_eq!(migrated["artifacts"][0]["profile"], legacy["profile"]);
    assert_eq!(migrated["artifacts"][0]["atlas"], legacy["knowledge_base"]);
}
