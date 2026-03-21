use cmx::scan::scan_source;
use std::fs;
use tempfile::TempDir;

fn write_file(dir: &std::path::Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).expect("write file");
}

fn agent_frontmatter(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

fn skill_frontmatter(description: &str) -> String {
    format!("---\ndescription: {description}\n---\n\n# Skill\n")
}

// --- Walk-based scan (no marketplace.json) ---

#[test]
fn scan_empty_directory_returns_empty() {
    let dir = TempDir::new().unwrap();
    let artifacts = scan_source(dir.path()).unwrap();
    assert!(artifacts.is_empty());
}

#[test]
fn scan_finds_agent_with_valid_frontmatter() {
    let dir = TempDir::new().unwrap();
    write_file(dir.path(), "my-agent.md", &agent_frontmatter("my-agent", "Does something"));
    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name(), "my-agent");
    assert_eq!(artifacts[0].description(), "Does something");
    assert_eq!(artifacts[0].kind(), "agent");
}

#[test]
fn scan_ignores_md_file_without_agent_frontmatter() {
    let dir = TempDir::new().unwrap();
    // Missing "name:" field — not an agent
    write_file(dir.path(), "readme.md", "---\ndescription: Just a readme\n---\n# Readme\n");
    let artifacts = scan_source(dir.path()).unwrap();
    assert!(artifacts.is_empty(), "expected empty, got {artifacts:?}");
}

#[test]
fn scan_ignores_md_file_without_any_frontmatter() {
    let dir = TempDir::new().unwrap();
    write_file(dir.path(), "notes.md", "# Notes\n\nJust notes.\n");
    let artifacts = scan_source(dir.path()).unwrap();
    assert!(artifacts.is_empty());
}

#[test]
fn scan_finds_skill_with_skill_md() {
    let dir = TempDir::new().unwrap();
    let skill_dir = dir.path().join("my-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    write_file(&skill_dir, "SKILL.md", &skill_frontmatter("A useful skill"));
    write_file(&skill_dir, "prompt.md", "# Prompt\n");

    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name(), "my-skill");
    assert_eq!(artifacts[0].description(), "A useful skill");
    assert_eq!(artifacts[0].kind(), "skill");
}

#[test]
fn scan_skips_hidden_directories() {
    let dir = TempDir::new().unwrap();
    let hidden = dir.path().join(".hidden");
    fs::create_dir_all(&hidden).unwrap();
    write_file(&hidden, "secret-agent.md", &agent_frontmatter("secret-agent", "Hidden"));

    let artifacts = scan_source(dir.path()).unwrap();
    assert!(artifacts.is_empty(), "hidden dirs must be skipped");
}

#[test]
fn scan_finds_multiple_agents_sorted_by_name() {
    let dir = TempDir::new().unwrap();
    write_file(dir.path(), "zebra.md", &agent_frontmatter("zebra", "Last"));
    write_file(dir.path(), "alpha.md", &agent_frontmatter("alpha", "First"));
    write_file(dir.path(), "middle.md", &agent_frontmatter("middle", "Middle"));

    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 3);
    let names: Vec<_> = artifacts.iter().map(|a| a.name()).collect();
    assert_eq!(names, ["alpha", "middle", "zebra"]);
}

#[test]
fn scan_finds_both_agents_and_skills() {
    let dir = TempDir::new().unwrap();
    write_file(dir.path(), "agent.md", &agent_frontmatter("agent", "An agent"));
    let skill_dir = dir.path().join("skill");
    fs::create_dir_all(&skill_dir).unwrap();
    write_file(&skill_dir, "SKILL.md", &skill_frontmatter("A skill"));

    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 2);
    let kinds: Vec<_> = artifacts.iter().map(|a| a.kind()).collect();
    assert!(kinds.contains(&"agent"));
    assert!(kinds.contains(&"skill"));
}

// --- Marketplace scan (with .claude-plugin/marketplace.json) ---

#[test]
fn scan_marketplace_finds_declared_agents() {
    let dir = TempDir::new().unwrap();

    // Write an agent file
    write_file(dir.path(), "my-agent.md", &agent_frontmatter("my-agent", "Marketplace agent"));

    // Write marketplace manifest
    let plugin_dir = dir.path().join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = serde_json::json!({
        "plugins": [{
            "agents": ["my-agent.md"],
            "skills": []
        }]
    });
    fs::write(plugin_dir.join("marketplace.json"), manifest.to_string()).unwrap();

    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name(), "my-agent");
    assert_eq!(artifacts[0].kind(), "agent");
}

#[test]
fn scan_marketplace_finds_declared_skills() {
    let dir = TempDir::new().unwrap();

    // Write a skill directory
    let skill_dir = dir.path().join("my-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    write_file(&skill_dir, "SKILL.md", &skill_frontmatter("Marketplace skill"));

    // Write marketplace manifest
    let plugin_dir = dir.path().join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = serde_json::json!({
        "plugins": [{
            "agents": [],
            "skills": ["my-skill"]
        }]
    });
    fs::write(plugin_dir.join("marketplace.json"), manifest.to_string()).unwrap();

    let artifacts = scan_source(dir.path()).unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name(), "my-skill");
    assert_eq!(artifacts[0].kind(), "skill");
}
