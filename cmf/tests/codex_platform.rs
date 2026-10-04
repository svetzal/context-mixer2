//! Codex install behavior at the cmf command boundary.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cmx_core::checksum;
use cmx_core::types::{ArtifactKind, LockFile};
use tempfile::TempDir;

const PROFILE: &str = "rust-shipping";
const HAND_AUTHORED: &[u8] = b"# Project guidance\nKeep this hand-authored file.\n";

struct Project {
    tmp: TempDir,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("temporary directory");
        let root = tmp.path().join("project");
        fs::create_dir_all(tmp.path().join("home")).expect("isolated home");
        fs::create_dir_all(&root).expect("project directory");
        fs::write(root.join("AGENTS.md"), HAND_AUTHORED).expect("hand-authored guidance");
        Self { tmp, root }
    }

    fn run(&self, surface: &str, apply: bool) -> Output {
        let atlas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference-atlas");
        let mut command = Command::new(env!("CARGO_BIN_EXE_cmf"));
        command
            .arg("--root")
            .arg(atlas)
            .args([
                "install",
                PROFILE,
                "--local",
                "--surface",
                surface,
                "--platform",
                "codex",
            ])
            .current_dir(&self.root)
            .env("HOME", self.tmp.path().join("home"));
        if apply {
            command.arg("--apply");
        }
        let output = command.output().expect("cmf runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output
    }

    fn assert_preview_only(&self, output: &Output, planned: &Path) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(&format!("codex → {} (install)", planned.display())), "{stdout}");
        assert!(stdout.contains("Re-run with --apply"), "{stdout}");
        assert!(!self.root.join(planned).exists(), "preview must not write the project artifact");
        assert!(
            !self.tmp.path().join("home").join(planned).exists(),
            "preview must not write a home artifact"
        );
        assert!(
            !self.root.join(".context-mixer").exists(),
            "preview must not write locks or manifest"
        );
        assert!(
            !self.tmp.path().join("home/.config/context-mixer").exists(),
            "preview must not write global config"
        );
        assert_eq!(fs::read(self.root.join("AGENTS.md")).unwrap(), HAND_AUTHORED);
    }

    fn lock_entry(&self, kind: ArtifactKind, installed: &Path) {
        let path = self.root.join(".context-mixer/cmx-lock-codex.json");
        let lock: LockFile = serde_json::from_slice(&fs::read(path).expect("Codex lock exists"))
            .expect("valid Codex lock");
        assert_eq!(lock.packages.len(), 1);
        let entry = &lock.packages["AGENTS"];
        assert_eq!(entry.artifact_type, kind);
        assert_eq!(entry.version.as_deref(), Some("0.3.0"));
        assert_eq!(entry.source.repo, "bundled:AGENTS");
        assert_eq!(
            entry.installed_checksum,
            checksum::checksum_artifact(installed, kind, &cmx_core::gateway::real::RealFilesystem)
                .unwrap()
        );
        assert_ne!(entry.source_checksum, "");
        assert!(self.root.join(".context-mixer/cmf-manifest.json").exists());
        assert!(!self.root.join(".context-mixer/cmx-lock.json").exists());
        assert_eq!(fs::read(self.root.join("AGENTS.md")).unwrap(), HAND_AUTHORED);
    }
}

#[test]
fn codex_agent_preview_and_apply_write_toml_without_touching_project_guidance() {
    let project = Project::new();
    let installed = project.root.join(".codex/agents/AGENTS.toml");
    project
        .assert_preview_only(&project.run("agent", false), Path::new(".codex/agents/AGENTS.toml"));
    let output = project.run("agent", true);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("codex → .codex/agents/AGENTS.toml (install)"), "{stdout}");
    let content = fs::read_to_string(&installed).expect("Codex TOML installed");
    let agent: toml::Value = toml::from_str(&content).expect("valid TOML");
    assert_eq!(agent["name"].as_str(), Some("AGENTS"));
    assert!(agent["developer_instructions"].as_str().is_some());
    project.lock_entry(ArtifactKind::Agent, &installed);
    assert!(!project.root.join(".claude/agents/AGENTS.md").exists());
}

#[test]
fn codex_skill_preview_and_apply_write_skill_md() {
    let project = Project::new();
    let installed = project.root.join(".agents/skills/AGENTS/SKILL.md");
    project.assert_preview_only(&project.run("skill", false), Path::new(".agents/skills/AGENTS"));
    let output = project.run("skill", true);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("codex → .agents/skills/AGENTS (install)"), "{stdout}");
    let content = fs::read_to_string(&installed).expect("Codex skill installed");
    assert!(content.starts_with("---\nname: AGENTS\n"));
    assert!(content.contains("Keep decisions in pure functions"));
    project.lock_entry(ArtifactKind::Skill, &project.root.join(".agents/skills/AGENTS"));
    assert!(!project.root.join(".claude/skills/AGENTS").exists());
}
