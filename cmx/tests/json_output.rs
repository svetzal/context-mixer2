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
    temp: TempDir,
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

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args).output().unwrap()
    }

    fn run_json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
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

fn assert_no_human_placeholders(value: &Value) {
    match value {
        Value::String(s) => assert!(
            !matches!(s.as_str(), "-" | " " | "✅" | "⚠️" | "⛔"),
            "JSON should not contain human placeholder strings: {s}"
        ),
        Value::Array(items) => {
            for item in items {
                assert_no_human_placeholders(item);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                assert_no_human_placeholders(item);
            }
        }
        _ => {}
    }
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
        temp,
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

fn versioned_skill(desc: &str, version: &str) -> String {
    format!("---\ndescription: {desc}\nversion: {version}\n---\n# focus-skill\n")
}

fn versioned_agent(version: &str) -> String {
    format!(
        "---\nname: rust-agent\ndescription: A Rust craftsperson agent.\nversion: {version}\n---\n# rust-agent\n"
    )
}

fn tracked_lock_entry(
    artifact_type: ArtifactKind,
    source_path: &str,
    source_checksum: String,
    installed_checksum: String,
) -> LockEntry {
    LockEntry {
        artifact_type,
        version: Some("1.0.0".to_string()),
        installed_at: "2026-07-01T10:00:00Z".to_string(),
        source: LockSource {
            repo: "guidelines".to_string(),
            path: source_path.to_string(),
        },
        source_checksum,
        installed_checksum,
    }
}

fn write_update_fixture_config(config_dir: &Path, source_root: &Path, platforms: Vec<Platform>) {
    write_json(
        &config_dir.join("sources.json"),
        &SourcesFile {
            version: 1,
            sources: BTreeMap::from([(
                "guidelines".to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(source_root.to_path_buf()),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some("2026-07-05T00:00:00Z".to_string()),
                },
            )]),
        },
    );
    write_json(
        &config_dir.join("config.json"),
        &CmxConfig {
            version: 1,
            llm: LlmConfig {
                gateway: LlmGatewayType::OpenAI,
                model: "gpt-5.4".to_string(),
            },
            home: None,
            external: vec![],
            platforms,
        },
    );
    write_json(&config_dir.join("sets.json"), &SetsFile::default());
}

fn write_single_lock(config_dir: &Path, filename: &str, name: &str, entry: LockEntry) {
    write_json(
        &config_dir.join(filename),
        &LockFile {
            version: 1,
            packages: BTreeMap::from([(name.to_string(), entry)]),
        },
    );
}

fn multi_platform_skill_update_fixture() -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let project = root.join("project");
    let config_dir = home.join(".config").join("context-mixer");
    let source_root = root.join("guidelines");
    let source_skill_dir = source_root.join("focus-skill");
    let source_skill_path = source_skill_dir.join("SKILL.md");
    let claude_skill_dir = home.join(".claude").join("skills").join("focus-skill");
    let codex_skill_dir = home.join(".agents").join("skills").join("focus-skill");
    let hermes_skill_dir = home.join(".hermes").join("skills").join("focus-skill");

    for dir in [
        &project,
        &config_dir,
        &source_skill_dir,
        &claude_skill_dir,
        &codex_skill_dir,
        &hermes_skill_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    fs::write(&source_skill_path, versioned_skill("A test skill", "1.0.0")).unwrap();
    fs::write(claude_skill_dir.join("SKILL.md"), versioned_skill("A test skill", "1.0.0")).unwrap();
    fs::write(codex_skill_dir.join("SKILL.md"), versioned_skill("A test skill", "1.0.0")).unwrap();
    fs::write(hermes_skill_dir.join("SKILL.md"), versioned_skill("A test skill", "1.0.0")).unwrap();

    let source_checksum = checksum_for(ArtifactKind::Skill, &source_skill_dir);
    let claude_checksum = checksum_for(ArtifactKind::Skill, &claude_skill_dir);
    let codex_checksum = checksum_for(ArtifactKind::Skill, &codex_skill_dir);
    let hermes_checksum = checksum_for(ArtifactKind::Skill, &hermes_skill_dir);

    write_update_fixture_config(
        &config_dir,
        &source_root,
        vec![Platform::Claude, Platform::Codex, Platform::Hermes],
    );
    write_single_lock(
        &config_dir,
        "cmx-lock.json",
        "focus-skill",
        tracked_lock_entry(
            ArtifactKind::Skill,
            "focus-skill",
            source_checksum.clone(),
            claude_checksum,
        ),
    );
    write_single_lock(
        &config_dir,
        "cmx-lock-codex.json",
        "focus-skill",
        tracked_lock_entry(
            ArtifactKind::Skill,
            "focus-skill",
            source_checksum.clone(),
            codex_checksum,
        ),
    );
    write_single_lock(
        &config_dir,
        "cmx-lock-hermes.json",
        "focus-skill",
        tracked_lock_entry(ArtifactKind::Skill, "focus-skill", source_checksum, hermes_checksum),
    );

    fs::write(&source_skill_path, versioned_skill("A test skill", "2.0.0")).unwrap();
    fs::write(codex_skill_dir.join("SKILL.md"), versioned_skill("Codex local edit", "1.1.0"))
        .unwrap();
    fs::write(hermes_skill_dir.join("SKILL.md"), versioned_skill("Hermes local edit", "1.2.0"))
        .unwrap();

    Fixture {
        temp,
        home,
        project,
        config_dir,
    }
}

fn multi_platform_agent_update_fixture() -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let project = root.join("project");
    let config_dir = home.join(".config").join("context-mixer");
    let source_root = root.join("guidelines");
    let source_agents_dir = source_root.join("agents");
    let source_agent_path = source_agents_dir.join("rust-agent.md");
    let claude_agent_path = home.join(".claude").join("agents").join("rust-agent.md");
    let cursor_agent_path = home.join(".cursor").join("agents").join("rust-agent.md");

    for dir in [
        &project,
        &config_dir,
        &source_agents_dir,
        claude_agent_path.parent().unwrap(),
        cursor_agent_path.parent().unwrap(),
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    fs::write(&source_agent_path, versioned_agent("1.0.0")).unwrap();
    fs::write(&claude_agent_path, versioned_agent("1.0.0")).unwrap();
    fs::write(&cursor_agent_path, versioned_agent("1.0.0")).unwrap();

    let source_checksum = checksum_for(ArtifactKind::Agent, &source_agent_path);
    let claude_checksum = checksum_for(ArtifactKind::Agent, &claude_agent_path);
    let cursor_checksum = checksum_for(ArtifactKind::Agent, &cursor_agent_path);

    write_update_fixture_config(
        &config_dir,
        &source_root,
        vec![Platform::Claude, Platform::Cursor],
    );
    write_single_lock(
        &config_dir,
        "cmx-lock.json",
        "rust-agent",
        tracked_lock_entry(
            ArtifactKind::Agent,
            "agents/rust-agent.md",
            source_checksum.clone(),
            claude_checksum,
        ),
    );
    write_single_lock(
        &config_dir,
        "cmx-lock-cursor.json",
        "rust-agent",
        tracked_lock_entry(
            ArtifactKind::Agent,
            "agents/rust-agent.md",
            source_checksum,
            cursor_checksum,
        ),
    );

    fs::write(&source_agent_path, versioned_agent("2.0.0")).unwrap();
    fs::write(&cursor_agent_path, versioned_agent("1.1.0")).unwrap();

    Fixture {
        temp,
        home,
        project,
        config_dir,
    }
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
        temp,
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
    assert_eq!(list_artifact["platforms"], serde_json::json!(["claude"]));
    assert_eq!(list_artifact["status"], "outdated");

    let outdated = fixture.run_json(&["outdated", "--json"]);
    assert_eq!(outdated["artifacts"][0]["name"], "rust-agent");
    assert_eq!(outdated["artifacts"][0]["status"], "outdated");
    assert_eq!(outdated["artifacts"][0]["locally_modified"], false);

    let search = fixture.run_json(&["search", "focus", "--json"]);
    assert_eq!(search["query"], "focus");
    assert_eq!(search["results"][0]["version"], "1.5.0");
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

#[test]
fn outdated_empty_state_human_output_is_successful() {
    let fixture = empty_fixture();
    let output = fixture.run(&["outdated"]);
    assert!(
        output.status.success(),
        "outdated should exit successfully when nothing is outdated\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Everything is up to date.\n");
}

#[test]
fn init_json_reports_drifted_and_forced_update_statuses() {
    let fixture = empty_fixture();
    let initial = fixture.run_json(&["init", "--json"]);
    assert_eq!(initial["targets"][0]["status"], "installed");

    let skill_md = fixture.home.join(".claude").join("skills").join("cmx").join("SKILL.md");
    fs::write(
        &skill_md,
        concat!(
            "---\n",
            "description: Locally edited.\n",
            "metadata:\n",
            "  version: \"",
            env!("CARGO_PKG_VERSION"),
            "\"\n",
            "  author: Test\n",
            "---\n",
            "# locally edited\n"
        ),
    )
    .unwrap();

    let skipped = fixture.run(&["init", "--json"]);
    assert!(
        !skipped.status.success(),
        "drift refusal should exit non-zero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&skipped.stdout),
        String::from_utf8_lossy(&skipped.stderr)
    );
    let skipped_json: Value = serde_json::from_slice(&skipped.stdout).unwrap();
    assert_eq!(skipped_json["targets"][0]["action"], "drifted_skip");
    assert_eq!(skipped_json["targets"][0]["status"], "skipped_drifted");

    let forced = fixture.run_json(&["init", "--force", "--json"]);
    assert_eq!(forced["targets"][0]["action"], "update");
    assert_eq!(forced["targets"][0]["status"], "updated");
}

#[test]
fn init_force_lists_discarded_file_paths_in_human_output() {
    let fixture = empty_fixture();
    let initial = fixture.run(&["init"]);
    assert!(
        initial.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&initial.stdout),
        String::from_utf8_lossy(&initial.stderr)
    );

    let skill_dir = fixture.home.join(".claude").join("skills").join("cmx");
    let skill_md = skill_dir.join("SKILL.md");
    let local_only = skill_dir.join("local-only.md");
    fs::write(
        &skill_md,
        concat!(
            "---\n",
            "description: Locally edited.\n",
            "metadata:\n",
            "  version: \"",
            env!("CARGO_PKG_VERSION"),
            "\"\n",
            "  author: Test\n",
            "---\n",
            "# locally edited\n"
        ),
    )
    .unwrap();
    fs::write(&local_only, "scratch notes\n").unwrap();

    let forced = fixture.run(&["init", "--force"]);
    assert!(
        forced.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&forced.stdout),
        String::from_utf8_lossy(&forced.stderr)
    );

    let stdout = String::from_utf8_lossy(&forced.stdout);
    assert!(stdout.contains("Discarding local modification:"), "{stdout}");
    assert!(stdout.contains(&skill_md.display().to_string()), "{stdout}");
    assert!(stdout.contains(&local_only.display().to_string()), "{stdout}");
    assert!(!local_only.exists(), "local-only file should be discarded on forced init");
}

#[test]
fn unversioned_json_uses_null_versions_and_enum_statuses() {
    let fixture = empty_fixture();
    let source_root = fixture.home.parent().unwrap().join("guidelines");
    let source_skill_dir = source_root.join("focus-skill");
    let installed_skill_dir = fixture.home.join(".claude").join("skills").join("focus-skill");

    fs::create_dir_all(&source_skill_dir).unwrap();
    fs::create_dir_all(&installed_skill_dir).unwrap();

    fs::write(
        source_skill_dir.join("SKILL.md"),
        concat!(
            "---\n",
            "description: Use this skill when you need focus.\n",
            "---\n",
            "# focus-skill\n",
            "updated source copy\n"
        ),
    )
    .unwrap();
    fs::write(
        installed_skill_dir.join("SKILL.md"),
        concat!(
            "---\n",
            "description: Use this skill when you need focus.\n",
            "---\n",
            "# focus-skill\n",
            "installed copy\n"
        ),
    )
    .unwrap();

    let installed_checksum = checksum_for(ArtifactKind::Skill, &installed_skill_dir);

    write_json(
        &fixture.config_dir.join("sources.json"),
        &SourcesFile {
            version: 1,
            sources: BTreeMap::from([(
                "guidelines".to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(source_root),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some("2026-07-05T00:00:00Z".to_string()),
                },
            )]),
        },
    );
    write_json(
        &fixture.config_dir.join("cmx-lock.json"),
        &LockFile {
            version: 1,
            packages: BTreeMap::from([(
                "focus-skill".to_string(),
                LockEntry {
                    artifact_type: ArtifactKind::Skill,
                    version: None,
                    installed_at: "2026-07-05T00:00:00Z".to_string(),
                    source: LockSource {
                        repo: "guidelines".to_string(),
                        path: "focus-skill".to_string(),
                    },
                    source_checksum: installed_checksum.clone(),
                    installed_checksum,
                },
            )]),
        },
    );

    let list = fixture.run_json(&["skill", "list", "--json"]);
    let list_artifact = &list["artifacts"][0];
    assert!(list_artifact["installed_version"].is_null());
    assert!(list_artifact["available_version"].is_null());
    assert_eq!(list_artifact["status"], "unversioned");

    let outdated = fixture.run_json(&["outdated", "--json"]);
    let outdated_artifact = &outdated["artifacts"][0];
    assert!(outdated_artifact["installed_version"].is_null());
    assert!(outdated_artifact["available_version"].is_null());
    assert_eq!(outdated_artifact["status"], "outdated");

    let search = fixture.run_json(&["search", "focus", "--json"]);
    assert!(search["results"][0]["version"].is_null());

    assert_no_human_placeholders(&list);
    assert_no_human_placeholders(&outdated);
    assert_no_human_placeholders(&search);
}

#[test]
fn skill_info_honors_cmx_platform_env() {
    let fixture = populated_fixture();
    let claude_skill_dir = fixture.home.join(".claude").join("skills").join("shared-skill");
    let codex_skill_dir = fixture.home.join(".agents").join("skills").join("shared-skill");

    fs::create_dir_all(&claude_skill_dir).unwrap();
    fs::create_dir_all(&codex_skill_dir).unwrap();

    fs::write(
        claude_skill_dir.join("SKILL.md"),
        concat!("---\n", "description: Claude copy.\n", "---\n", "# shared-skill\n"),
    )
    .unwrap();
    fs::write(
        codex_skill_dir.join("SKILL.md"),
        concat!("---\n", "description: Codex copy.\n", "---\n", "# shared-skill\n"),
    )
    .unwrap();

    let default_info = fixture.run_json(&["skill", "info", "shared-skill", "--json"]);
    assert_eq!(default_info["activation_description"], "Claude copy.");
    assert!(
        default_info["path"].as_str().unwrap().contains("/.claude/skills/shared-skill"),
        "default lookup should prefer the default active platform"
    );

    let env_output = fixture
        .command(&["skill", "info", "shared-skill", "--json"])
        .env("CMX_PLATFORM", "codex")
        .output()
        .unwrap();
    assert!(
        env_output.status.success(),
        "CMX_PLATFORM=codex should prefer the Codex-visible copy\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&env_output.stdout),
        String::from_utf8_lossy(&env_output.stderr)
    );

    let info: Value = serde_json::from_slice(&env_output.stdout).unwrap();
    assert_eq!(info["name"], "shared-skill");
    assert_eq!(info["kind"], "skill");
    assert_eq!(info["activation_description"], "Codex copy.");
    assert!(
        info["path"].as_str().unwrap().contains("/.agents/skills/shared-skill"),
        "CMX_PLATFORM=codex should change the preferred copy"
    );
}

#[test]
fn adopt_from_dir_succeeds_without_deprecation_warning() {
    let fixture = empty_fixture();
    let orphan_dir = fixture.home.join(".claude").join("skills").join("focus-skill");
    fs::create_dir_all(&orphan_dir).unwrap();
    fs::write(
        orphan_dir.join("SKILL.md"),
        concat!(
            "---\n",
            "description: A hand-authored skill.\n",
            "version: 1.0.0\n",
            "---\n",
            "# focus-skill\n"
        ),
    )
    .unwrap();

    let orphan_dir_arg = orphan_dir.parent().unwrap().display().to_string();
    let output = fixture.run(&["skill", "adopt", "--all", "--from-dir", &orphan_dir_arg]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("--from is deprecated; use --from-dir"), "{stdout}");
    assert!(!stderr.contains("--from is deprecated; use --from-dir"), "{stderr}");
    assert!(fixture.config_dir.join("home/skills/focus-skill/SKILL.md").exists());
}

#[test]
fn adopt_deprecated_from_warns_on_stderr_only() {
    let fixture = empty_fixture();
    let orphan_dir = fixture.home.join(".claude").join("skills").join("focus-skill");
    fs::create_dir_all(&orphan_dir).unwrap();
    fs::write(
        orphan_dir.join("SKILL.md"),
        concat!(
            "---\n",
            "description: A hand-authored skill.\n",
            "version: 1.0.0\n",
            "---\n",
            "# focus-skill\n"
        ),
    )
    .unwrap();

    let orphan_dir_arg = orphan_dir.parent().unwrap().display().to_string();
    let output = fixture.run(&["skill", "adopt", "--all", "--from", &orphan_dir_arg]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("--from is deprecated; use --from-dir"), "{stdout}");
    assert!(stderr.contains("--from is deprecated; use --from-dir"), "{stderr}");
    assert!(fixture.config_dir.join("home/skills/focus-skill/SKILL.md").exists());
}

#[test]
fn uninstall_without_args_prints_try_line_on_stderr() {
    let fixture = populated_fixture();
    let output = fixture.run(&["skill", "uninstall"]);
    assert!(!output.status.success(), "command should fail without args");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Provide artifact name(s) to uninstall"), "{stderr}");
    assert!(stderr.contains("try: cmx skill uninstall <name>"), "{stderr}");
}

#[test]
fn info_near_miss_suggests_closest_artifact() {
    let fixture = populated_fixture();
    let output = fixture.run(&["info", "focus-skll"]);
    assert!(!output.status.success(), "unknown artifact should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("focus-skll"), "{stderr}");
    assert!(stderr.contains("Did you mean 'focus-skill'?"), "{stderr}");
}

#[test]
fn set_create_from_plugin_succeeds_without_deprecation_warning() {
    let fixture = populated_fixture();
    let source_root = fixture.home.parent().unwrap().join("guidelines");
    fs::create_dir_all(source_root.join(".claude-plugin")).unwrap();
    fs::write(
        source_root.join(".claude-plugin").join("marketplace.json"),
        r#"{"name":"test","plugins":[{"name":"workbench","agents":["./agents/rust-agent.md"],"skills":["./focus-skill"]}]}"#,
    )
    .unwrap();

    let output = fixture.run(&[
        "set",
        "create",
        "seeded",
        "--from-plugin",
        "guidelines:workbench",
    ]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("--from is deprecated; use --from-plugin"), "{stdout}");
    assert!(!stderr.contains("--from is deprecated; use --from-plugin"), "{stderr}");

    let sets: SetsFile =
        serde_json::from_slice(&fs::read(fixture.config_dir.join("sets.json")).unwrap()).unwrap();
    let seeded = sets.sets.get("seeded").expect("set created");
    assert_eq!(seeded.members.len(), 2);
}

#[test]
fn set_create_deprecated_from_warns_on_stderr_only() {
    let fixture = populated_fixture();
    let source_root = fixture.home.parent().unwrap().join("guidelines");
    fs::create_dir_all(source_root.join(".claude-plugin")).unwrap();
    fs::write(
        source_root.join(".claude-plugin").join("marketplace.json"),
        r#"{"name":"test","plugins":[{"name":"workbench","agents":["./agents/rust-agent.md"],"skills":["./focus-skill"]}]}"#,
    )
    .unwrap();

    let output = fixture.run(&["set", "create", "seeded", "--from", "guidelines:workbench"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("--from is deprecated; use --from-plugin"), "{stdout}");
    assert!(stderr.contains("--from is deprecated; use --from-plugin"), "{stderr}");

    let sets: SetsFile =
        serde_json::from_slice(&fs::read(fixture.config_dir.join("sets.json")).unwrap()).unwrap();
    let seeded = sets.sets.get("seeded").expect("set created");
    assert_eq!(seeded.members.len(), 2);
}

#[test]
fn set_activate_deprecated_dry_run_warns_on_stderr_only() {
    let fixture = populated_fixture();
    let mut sets = BTreeMap::new();
    sets.insert(
        "focus".to_string(),
        SetDef {
            description: None,
            state: SetState::Inactive,
            members: vec![SetMember {
                kind: ArtifactKind::Skill,
                name: "focus-skill".to_string(),
                source: Some("guidelines".to_string()),
            }],
        },
    );
    write_json(&fixture.config_dir.join("sets.json"), &SetsFile { version: 1, sets });

    let output = fixture.run(&["set", "activate", "focus", "--dry-run"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let warning =
        "--dry-run is deprecated; the plan is now shown by default — pass --apply to execute";
    assert!(!stdout.contains(warning), "{stdout}");
    assert!(stderr.contains(warning), "{stderr}");
    assert!(stdout.contains("Re-run with --apply to make these changes."), "{stdout}");

    let sets: SetsFile =
        serde_json::from_slice(&fs::read(fixture.config_dir.join("sets.json")).unwrap()).unwrap();
    assert_eq!(sets.sets.get("focus").unwrap().state, SetState::Inactive);
}

#[test]
fn update_force_lists_discarded_file_paths() {
    let fixture = populated_fixture();
    let skill_dir = fixture.home.join(".claude").join("skills").join("focus-skill");
    let skill_md = skill_dir.join("SKILL.md");
    let extra = skill_dir.join("local-only.md");
    fs::write(
        &skill_md,
        concat!(
            "---\n",
            "description: Locally edited.\n",
            "version: 2.0.0\n",
            "---\n",
            "# locally edited\n"
        ),
    )
    .unwrap();
    fs::write(&extra, "local notes\n").unwrap();

    let output = fixture.run(&["skill", "update", "focus-skill", "--force"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&skill_md.display().to_string()), "{stdout}");
    assert!(stdout.contains(&extra.display().to_string()), "{stdout}");
    assert!(!extra.exists(), "local-only file should be discarded on forced update");
}

#[test]
fn skill_update_warns_about_drifted_sibling_platforms_on_stderr() {
    let fixture = multi_platform_skill_update_fixture();
    let output = fixture.run(&["skill", "update", "focus-skill", "--force"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("Installed focus-skill"), "{stdout}");
    assert!(stdout.contains("for claude"), "{stdout}");
    assert!(stderr.contains("codex, hermes"), "{stderr}");
    assert!(stderr.contains("cmx skill sync focus-skill"), "{stderr}");
    assert!(stderr.contains("'update' only targets claude"), "{stderr}");
}

#[test]
fn skill_update_without_sibling_installs_emits_no_warning_note() {
    let fixture = populated_fixture();
    let source_skill = fixture.temp.path().join("guidelines").join("focus-skill").join("SKILL.md");
    fs::write(&source_skill, versioned_skill("Updated focus", "2.0.0")).unwrap();

    let output = fixture.run(&["skill", "update", "focus-skill"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("'update' only targets"), "{stderr}");
}

#[test]
fn agent_update_warns_about_drifted_sibling_platforms_on_stderr() {
    let fixture = multi_platform_agent_update_fixture();
    let output = fixture.run(&["agent", "update", "rust-agent"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("Installed rust-agent"), "{stdout}");
    assert!(stdout.contains("for claude"), "{stdout}");
    assert!(stderr.contains("cursor"), "{stderr}");
    assert!(
        stderr.contains("cmx agent update rust-agent --platform <platform> --force"),
        "{stderr}"
    );
    assert!(stderr.contains("'update' only targets claude"), "{stderr}");
}
