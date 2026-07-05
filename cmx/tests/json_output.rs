use cmx::checksum;
use cmx::gateway::real::RealFilesystem;
use cmx::platform::Platform;
use cmx::types::{
    ArtifactKind, CmxConfig, LlmConfig, LlmGatewayType, LockEntry, LockFile, LockSource, SetDef,
    SetMember, SetState, SetsFile, SourceEntry, SourceType, SourcesFile,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_cmx");

struct Fixture {
    _temp: TempDir,
    home: PathBuf,
    project: PathBuf,
    config_dir: PathBuf,
}

struct FixturePaths {
    home: PathBuf,
    project: PathBuf,
    config_dir: PathBuf,
    source_root: PathBuf,
    source_agent_path: PathBuf,
    source_skill_dir: PathBuf,
    source_skill_path: PathBuf,
    installed_agent_path: PathBuf,
    installed_skill_dir: PathBuf,
    installed_skill_path: PathBuf,
}

struct FixtureChecksums {
    agent_source: String,
    agent_installed: String,
    skill_installed: String,
}

impl Fixture {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command
            .args(args)
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("OPENAI_API_KEY", "");
        command
    }

    fn run_json(&self, args: &[&str]) -> Value {
        let output = self.command(args).output().unwrap();
        assert!(
            output.status.success(),
            "command {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "stdout for {:?} was not valid JSON: {error}\nstdout:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn checksum_for(kind: ArtifactKind, path: &Path) -> String {
    checksum::checksum_artifact(path, kind, &RealFilesystem).unwrap()
}

fn populated_fixture() -> Fixture {
    let temp = TempDir::new().unwrap();
    let paths = create_fixture_paths(temp.path());
    write_populated_artifacts(&paths);
    let checksums = compute_fixture_checksums(&paths);
    write_populated_config(&paths, &checksums);

    assert!(!checksums.agent_source.is_empty());

    Fixture {
        _temp: temp,
        home: paths.home,
        project: paths.project,
        config_dir: paths.config_dir,
    }
}

fn create_fixture_paths(root: &Path) -> FixturePaths {
    let home = root.join("home");
    let project = root.join("project");
    let config_dir = home.join(".config").join("context-mixer");
    let source_root = root.join("guidelines");
    let source_agent_path = source_root.join("agents").join("rust-agent.md");
    let source_skill_dir = source_root.join("focus-skill");
    let source_skill_path = source_skill_dir.join("SKILL.md");
    let installed_agent_path = home.join(".claude").join("agents").join("rust-agent.md");
    let installed_skill_dir = home.join(".claude").join("skills").join("focus-skill");
    let installed_skill_path = installed_skill_dir.join("SKILL.md");

    for dir in [
        &project,
        &config_dir,
        &source_root.join("agents"),
        &source_skill_dir,
        &home.join(".claude").join("agents"),
        &installed_skill_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    FixturePaths {
        home,
        project,
        config_dir,
        source_root,
        source_agent_path,
        source_skill_dir,
        source_skill_path,
        installed_agent_path,
        installed_skill_dir,
        installed_skill_path,
    }
}

fn write_populated_artifacts(paths: &FixturePaths) {
    fs::write(
        &paths.source_agent_path,
        concat!(
            "---\n",
            "name: rust-agent\n",
            "description: A Rust craftsperson agent.\n",
            "version: 2.0.0\n",
            "---\n",
            "# rust-agent\n"
        ),
    )
    .unwrap();
    fs::write(
        &paths.source_skill_path,
        concat!(
            "---\n",
            "description: Use this skill when you need sustained focus during deep work sessions while triaging multiple threads calmly and deliberately.\n",
            "version: 1.5.0\n",
            "---\n",
            "# focus-skill\n"
        ),
    )
    .unwrap();
    fs::write(paths.source_skill_dir.join("notes.md"), "# Notes\n").unwrap();

    fs::write(
        &paths.installed_agent_path,
        concat!(
            "---\n",
            "name: rust-agent\n",
            "description: A Rust craftsperson agent.\n",
            "version: 1.0.0\n",
            "---\n",
            "# rust-agent\n"
        ),
    )
    .unwrap();
    fs::write(
        &paths.installed_skill_path,
        fs::read_to_string(&paths.source_skill_path).unwrap(),
    )
    .unwrap();
    fs::write(paths.installed_skill_dir.join("notes.md"), "# Notes\n").unwrap();
}

fn compute_fixture_checksums(paths: &FixturePaths) -> FixtureChecksums {
    FixtureChecksums {
        agent_source: checksum_for(ArtifactKind::Agent, &paths.source_agent_path),
        agent_installed: checksum_for(ArtifactKind::Agent, &paths.installed_agent_path),
        skill_installed: checksum_for(ArtifactKind::Skill, &paths.installed_skill_dir),
    }
}

fn write_populated_config(paths: &FixturePaths, checksums: &FixtureChecksums) {
    write_sources_file(paths);
    write_config_file(paths);
    write_lock_file(paths, checksums);
    write_sets_file(paths);
}

fn write_sources_file(paths: &FixturePaths) {
    write_json(
        &paths.config_dir.join("sources.json"),
        &SourcesFile {
            version: 1,
            sources: BTreeMap::from([(
                "guidelines".to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(paths.source_root.clone()),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some("2026-07-05T00:00:00Z".to_string()),
                },
            )]),
        },
    );
}

fn write_config_file(paths: &FixturePaths) {
    write_json(
        &paths.config_dir.join("config.json"),
        &CmxConfig {
            version: 1,
            llm: LlmConfig {
                gateway: LlmGatewayType::OpenAI,
                model: "gpt-5.4".to_string(),
            },
            home: None,
            external: vec!["~/.hermes/skills".to_string()],
            platforms: vec![Platform::Claude, Platform::Codex],
        },
    );
}

fn write_lock_file(paths: &FixturePaths, checksums: &FixtureChecksums) {
    write_json(
        &paths.config_dir.join("cmx-lock.json"),
        &LockFile {
            version: 1,
            packages: BTreeMap::from([
                (
                    "rust-agent".to_string(),
                    LockEntry {
                        artifact_type: ArtifactKind::Agent,
                        version: Some("1.0.0".to_string()),
                        installed_at: "2026-07-01T10:00:00Z".to_string(),
                        source: LockSource {
                            repo: "guidelines".to_string(),
                            path: "agents/rust-agent.md".to_string(),
                        },
                        source_checksum: checksums.agent_installed.clone(),
                        installed_checksum: checksums.agent_installed.clone(),
                    },
                ),
                (
                    "focus-skill".to_string(),
                    LockEntry {
                        artifact_type: ArtifactKind::Skill,
                        version: Some("1.5.0".to_string()),
                        installed_at: "2026-07-02T11:00:00Z".to_string(),
                        source: LockSource {
                            repo: "guidelines".to_string(),
                            path: "focus-skill".to_string(),
                        },
                        source_checksum: checksums.skill_installed.clone(),
                        installed_checksum: checksums.skill_installed.clone(),
                    },
                ),
            ]),
        },
    );
}

fn write_sets_file(paths: &FixturePaths) {
    write_json(
        &paths.config_dir.join("sets.json"),
        &SetsFile {
            version: 1,
            sets: BTreeMap::from([(
                "daily".to_string(),
                SetDef {
                    description: Some("Daily tools".to_string()),
                    state: SetState::Active,
                    members: vec![
                        SetMember {
                            kind: ArtifactKind::Agent,
                            name: "rust-agent".to_string(),
                            source: Some("guidelines".to_string()),
                        },
                        SetMember {
                            kind: ArtifactKind::Skill,
                            name: "focus-skill".to_string(),
                            source: Some("guidelines".to_string()),
                        },
                    ],
                },
            )]),
        },
    );
}

fn empty_fixture() -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let project = root.join("project");
    let config_dir = home.join(".config").join("context-mixer");
    let empty_source = root.join("empty-source");

    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&empty_source).unwrap();

    write_json(&config_dir.join("sources.json"), &SourcesFile::default());
    write_json(&config_dir.join("config.json"), &CmxConfig::default());
    write_json(&config_dir.join("cmx-lock.json"), &LockFile::default());
    write_json(&config_dir.join("sets.json"), &SetsFile::default());

    Fixture {
        _temp: temp,
        home,
        project,
        config_dir,
    }
}

#[test]
fn data_reporting_commands_accept_json_and_emit_expected_fields() {
    let fixture = populated_fixture();

    for args in [
        vec!["list", "--json"],
        vec!["list", "--all", "--json"],
        vec!["skill", "list", "--json"],
        vec!["skill", "list", "--all", "--json"],
        vec!["agent", "list", "--json"],
        vec!["agent", "list", "--all", "--json"],
        vec!["outdated", "--json"],
        vec!["search", "focus", "--json"],
        vec!["info", "focus-skill", "--json"],
        vec!["skill", "info", "focus-skill", "--json"],
        vec!["agent", "info", "rust-agent", "--json"],
        vec!["source", "list", "--json"],
        vec!["source", "browse", "guidelines", "--json"],
        vec!["set", "list", "--json"],
        vec!["set", "show", "daily", "--json"],
        vec!["config", "show", "--json"],
        vec!["home", "path", "--json"],
    ] {
        let _ = fixture.run_json(&args);
    }

    let list = fixture.run_json(&["list", "--json"]);
    let list_artifact = list["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "rust-agent")
        .unwrap();
    assert_eq!(list_artifact["kind"], "agent");
    assert_eq!(list_artifact["scope"], "global");
    assert_eq!(list_artifact["available_version"], "2.0.0");
    assert_eq!(list_artifact["tools"], serde_json::json!(["claude"]));

    let outdated = fixture.run_json(&["outdated", "--json"]);
    assert_eq!(outdated["artifacts"][0]["name"], "rust-agent");
    assert_eq!(outdated["artifacts"][0]["status"], "update");

    let search = fixture.run_json(&["search", "focus", "--json"]);
    assert_eq!(search["query"], "focus");
    assert!(
        search["results"][0]["description"]
            .as_str()
            .unwrap()
            .contains("triaging multiple threads calmly"),
        "search JSON should keep the full description"
    );

    let info = fixture.run_json(&["info", "focus-skill", "--json"]);
    assert_eq!(info["name"], "focus-skill");
    assert_eq!(info["kind"], "skill");
    assert_eq!(info["scope"], "global");
    assert_eq!(info["version"], "1.5.0");
    assert_eq!(info["source"], "guidelines (focus-skill)");
    assert_eq!(
        info["activation_description"],
        "Use this skill when you need sustained focus during deep work sessions while triaging multiple threads calmly and deliberately."
    );
    assert!(info["summary"].is_null());
    assert_eq!(info["files"][0]["name"], "SKILL.md");

    let agent_info = fixture.run_json(&["agent", "info", "rust-agent", "--json"]);
    assert_eq!(agent_info["kind"], "agent");
    assert_eq!(agent_info["available_version"], "2.0.0");

    let source_list = fixture.run_json(&["source", "list", "--json"]);
    assert_eq!(source_list["sources"][0]["name"], "guidelines");
    assert_eq!(source_list["sources"][0]["type"], "local");

    let source_browse = fixture.run_json(&["source", "browse", "guidelines", "--json"]);
    assert_eq!(source_browse["source"], "guidelines");
    assert_eq!(source_browse["skills"][0]["name"], "focus-skill");
    assert_eq!(source_browse["skills"][0]["files"], serde_json::json!(["SKILL.md", "notes.md"]));
    assert!(
        source_browse["skills"][0]["description"]
            .as_str()
            .unwrap()
            .contains("triaging multiple threads calmly")
    );

    let set_list = fixture.run_json(&["set", "list", "--json"]);
    assert_eq!(set_list["scope"], "global");
    assert_eq!(set_list["sets"][0]["name"], "daily");
    assert_eq!(set_list["sets"][0]["state"], "active");

    let set_show = fixture.run_json(&["set", "show", "daily", "--json"]);
    assert_eq!(set_show["name"], "daily");
    assert_eq!(set_show["description"], "Daily tools");
    assert_eq!(set_show["members"][0]["source"], "guidelines");
    assert_eq!(set_show["members"][0]["installed"], true);

    let config = fixture.run_json(&["config", "show", "--json"]);
    assert_eq!(config["gateway"], "openai");
    assert_eq!(config["platforms"], serde_json::json!(["claude", "codex"]));
    assert_eq!(config["platforms_inferred"], false);

    let home = fixture.run_json(&["home", "path", "--json"]);
    assert_eq!(home["path"], fixture.config_dir.join("home").display().to_string());
}

#[test]
fn empty_state_json_is_valid_and_machine_readable() {
    let fixture = empty_fixture();

    let list = fixture.run_json(&["list", "--json"]);
    assert_eq!(list["artifacts"], serde_json::json!([]));

    let agent_list = fixture.run_json(&["agent", "list", "--json"]);
    assert_eq!(agent_list["artifacts"], serde_json::json!([]));

    let skill_list = fixture.run_json(&["skill", "list", "--json"]);
    assert_eq!(skill_list["artifacts"], serde_json::json!([]));

    let outdated = fixture.run_json(&["outdated", "--json"]);
    assert_eq!(outdated["artifacts"], serde_json::json!([]));

    let search = fixture.run_json(&["search", "missing", "--json"]);
    assert_eq!(search["results"], serde_json::json!([]));

    let source_list = fixture.run_json(&["source", "list", "--json"]);
    assert_eq!(source_list["sources"], serde_json::json!([]));

    write_json(
        &fixture.config_dir.join("sources.json"),
        &SourcesFile {
            version: 1,
            sources: BTreeMap::from([(
                "empty".to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(fixture.home.parent().unwrap().join("empty-source")),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some("2026-07-05T00:00:00Z".to_string()),
                },
            )]),
        },
    );

    let source_browse = fixture.run_json(&["source", "browse", "empty", "--json"]);
    assert_eq!(source_browse["agents"], serde_json::json!([]));
    assert_eq!(source_browse["skills"], serde_json::json!([]));

    let set_list = fixture.run_json(&["set", "list", "--json"]);
    assert_eq!(set_list["sets"], serde_json::json!([]));

    write_json(
        &fixture.config_dir.join("sets.json"),
        &SetsFile {
            version: 1,
            sets: BTreeMap::from([(
                "empty-set".to_string(),
                SetDef {
                    description: None,
                    state: SetState::Inactive,
                    members: vec![],
                },
            )]),
        },
    );

    let set_show = fixture.run_json(&["set", "show", "empty-set", "--json"]);
    assert_eq!(set_show["members"], serde_json::json!([]));

    let config = fixture.run_json(&["config", "show", "--json"]);
    assert_eq!(config["external"], serde_json::json!([]));

    let home = fixture.run_json(&["home", "path", "--json"]);
    assert!(home["path"].as_str().unwrap().ends_with(".config/context-mixer/home"));
}
