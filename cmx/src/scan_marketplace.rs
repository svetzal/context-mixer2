use anyhow::Result;
use std::path::Path;

use crate::gateway::Filesystem;
use crate::scan::{ScanWarning, try_parse_artifact, walk_dir_with};
use crate::types::Artifact;
use crate::types::ArtifactKind;

pub(crate) fn scan_marketplace_with(
    root: &Path,
    marketplace_path: &Path,
    fs: &dyn Filesystem,
    warnings: &mut Vec<ScanWarning>,
) -> Result<Vec<Artifact>> {
    let content = fs.read_to_string(marketplace_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let mut artifacts = Vec::new();

    if let Some(plugins) = manifest.get("plugins").and_then(|p| p.as_array()) {
        for plugin in plugins {
            let has_agents =
                plugin.get("agents").and_then(|a| a.as_array()).is_some_and(|a| !a.is_empty());
            let has_skills =
                plugin.get("skills").and_then(|s| s.as_array()).is_some_and(|s| !s.is_empty());

            // If explicit agents/skills arrays are present, use them directly
            if has_agents || has_skills {
                scan_marketplace_explicit_arrays(root, plugin, &mut artifacts, fs, warnings);
                continue;
            }

            // Otherwise, resolve the source field and walk the directory
            if let Some(source) = plugin.get("source") {
                if let Some(source_path) = source.as_str() {
                    // Relative path source — walk the resolved directory
                    let resolved = root.join(source_path);
                    if fs.exists(&resolved) {
                        walk_dir_with(&resolved, &mut artifacts, fs)?;
                    } else {
                        let plugin_name =
                            plugin.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>");
                        warnings.push(ScanWarning {
                            message: format!(
                                "plugin '{plugin_name}' source path '{source_path}' does not exist"
                            ),
                        });
                    }
                } else if source.is_object() {
                    // Remote source (github, url, git-subdir, npm) — not yet supported
                    let plugin_name =
                        plugin.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>");
                    let source_type =
                        source.get("source").and_then(|s| s.as_str()).unwrap_or("unknown");
                    warnings.push(ScanWarning {
                        message: format!(
                            "plugin '{plugin_name}' uses remote source type '{source_type}' which is not yet supported"
                        ),
                    });
                }
            }
        }
    }

    Ok(artifacts)
}

fn scan_marketplace_explicit_arrays(
    root: &Path,
    plugin: &serde_json::Value,
    artifacts: &mut Vec<Artifact>,
    fs: &dyn Filesystem,
    warnings: &mut Vec<ScanWarning>,
) {
    // Scan declared agents
    if let Some(agents) = plugin.get("agents").and_then(|a| a.as_array()) {
        for agent_path in agents {
            if let Some(path_str) = agent_path.as_str() {
                let full_path = root.join(path_str);
                if !fs.exists(&full_path) {
                    warnings.push(ScanWarning {
                        message: format!(
                            "marketplace declares agent '{path_str}' but path does not exist"
                        ),
                    });
                    continue;
                }
                if let Some(artifact) = try_parse_artifact(ArtifactKind::Agent, &full_path, fs) {
                    artifacts.push(artifact);
                }
            }
        }
    }

    // Scan declared skills
    if let Some(skills) = plugin.get("skills").and_then(|s| s.as_array()) {
        for skill_path in skills {
            if let Some(path_str) = skill_path.as_str() {
                let full_path = root.join(path_str);
                if !fs.exists(&full_path) {
                    warnings.push(ScanWarning {
                        message: format!(
                            "marketplace declares skill '{path_str}' but path does not exist"
                        ),
                    });
                    continue;
                }
                let skill_md = full_path.join("SKILL.md");
                if !fs.exists(&skill_md) {
                    warnings.push(ScanWarning {
                        message: format!(
                            "marketplace declares skill '{path_str}' but SKILL.md is missing"
                        ),
                    });
                    continue;
                }
                if let Some(artifact) = try_parse_artifact(ArtifactKind::Skill, &full_path, fs) {
                    artifacts.push(artifact);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{agent_content, skill_content};

    fn marketplace_json(plugins_json: &str) -> String {
        format!(r#"{{"name":"test","plugins":[{plugins_json}]}}"#)
    }

    fn run(
        fs: &FakeFilesystem,
        root: &str,
        marketplace: &str,
    ) -> (Vec<Artifact>, Vec<ScanWarning>) {
        let mut warnings = Vec::new();
        let artifacts =
            scan_marketplace_with(Path::new(root), Path::new(marketplace), fs, &mut warnings)
                .unwrap();
        (artifacts, warnings)
    }

    #[test]
    fn empty_plugins_array_returns_no_artifacts_and_no_warnings() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"test","plugins":[]}"#);
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_plugins_key_returns_no_artifacts_and_no_warnings() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"test"}"#);
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn plugin_with_relative_source_path_walks_directory() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","source":"./plugins/my-plugin"}"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/agents/reviewer.md",
            agent_content("reviewer", "Reviews code"),
        );
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "reviewer");
        assert!(warnings.is_empty());
    }

    #[test]
    fn plugin_with_non_existent_source_path_produces_warning() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"ghost-plugin","source":"./nonexistent"}"#),
        );
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("ghost-plugin"),
            "unexpected: {}",
            warnings[0].message
        );
        assert!(
            warnings[0].message.contains("does not exist"),
            "unexpected: {}",
            warnings[0].message
        );
    }

    #[test]
    fn plugin_with_object_source_produces_not_yet_supported_warning() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"remote-plugin","source":{"source":"url","url":"https://example.com"}}"#,
            ),
        );
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("not yet supported"),
            "unexpected: {}",
            warnings[0].message
        );
    }

    #[test]
    fn plugin_with_explicit_agents_array_returns_agent_artifacts() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","agents":["./agents/my-agent.md"]}"#),
        );
        fs.add_file("/repo/agents/my-agent.md", agent_content("my-agent", "Does things"));
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "my-agent");
        assert_eq!(artifacts[0].kind, ArtifactKind::Agent);
        assert!(warnings.is_empty());
    }

    #[test]
    fn plugin_with_explicit_skills_array_returns_skill_artifacts() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","skills":["./skills/my-skill"]}"#),
        );
        fs.add_file("/repo/skills/my-skill/SKILL.md", skill_content("A skill"));
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "my-skill");
        assert_eq!(artifacts[0].kind, ArtifactKind::Skill);
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_agent_path_in_explicit_array_produces_warning_and_continues() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"my-plugin","agents":["./agents/missing.md","./agents/present.md"]}"#,
            ),
        );
        fs.add_file("/repo/agents/present.md", agent_content("present", "Present agent"));
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "present");
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("missing.md"),
            "unexpected: {}",
            warnings[0].message
        );
    }

    #[test]
    fn missing_skill_path_in_explicit_array_produces_warning_and_skips() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","skills":["./skills/ghost"]}"#),
        );
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("ghost"), "unexpected: {}", warnings[0].message);
    }

    #[test]
    fn skill_path_exists_but_skill_md_missing_produces_warning() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","skills":["./skills/my-skill"]}"#),
        );
        fs.add_file("/repo/skills/my-skill/tool.py", "code");
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert!(artifacts.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("SKILL.md is missing"),
            "unexpected: {}",
            warnings[0].message
        );
    }

    #[test]
    fn plugin_with_both_agents_and_skills_returns_both_kinds() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"my-plugin","agents":["./agents/my-agent.md"],"skills":["./skills/my-skill"]}"#,
            ),
        );
        fs.add_file("/repo/agents/my-agent.md", agent_content("my-agent", "An agent"));
        fs.add_file("/repo/skills/my-skill/SKILL.md", skill_content("A skill"));
        let (artifacts, warnings) = run(&fs, "/repo", "/repo/.claude-plugin/marketplace.json");
        assert_eq!(artifacts.len(), 2);
        assert!(warnings.is_empty());
        let kinds: Vec<_> = artifacts.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&ArtifactKind::Agent));
        assert!(kinds.contains(&ArtifactKind::Skill));
    }
}
