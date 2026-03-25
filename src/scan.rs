use anyhow::Result;
use std::path::Path;

use crate::gateway::filesystem::Filesystem;
use crate::types::{Artifact, ArtifactKind, Deprecation};

// ---------------------------------------------------------------------------
// Testable variant (accepts injected Filesystem)
// ---------------------------------------------------------------------------

pub fn scan_source_with(root: &Path, fs: &dyn Filesystem) -> Result<Vec<Artifact>> {
    let marketplace = root.join(".claude-plugin").join("marketplace.json");

    let mut artifacts = if fs.exists(&marketplace) {
        scan_marketplace_with(root, &marketplace, fs)?
    } else {
        let mut arts = Vec::new();
        walk_dir_with(root, &mut arts, fs)?;
        arts
    };

    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(artifacts)
}

fn scan_marketplace_with(
    root: &Path,
    marketplace_path: &Path,
    fs: &dyn Filesystem,
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
                scan_marketplace_explicit_arrays(root, plugin, &mut artifacts, fs);
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
                        eprintln!(
                            "Warning: plugin '{plugin_name}' source path '{source_path}' does not exist"
                        );
                    }
                } else if source.is_object() {
                    // Remote source (github, url, git-subdir, npm) — not yet supported
                    let plugin_name =
                        plugin.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>");
                    let source_type =
                        source.get("source").and_then(|s| s.as_str()).unwrap_or("unknown");
                    eprintln!(
                        "Warning: plugin '{plugin_name}' uses remote source type '{source_type}' which is not yet supported"
                    );
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
) {
    // Scan declared agents
    if let Some(agents) = plugin.get("agents").and_then(|a| a.as_array()) {
        for agent_path in agents {
            if let Some(path_str) = agent_path.as_str() {
                let full_path = root.join(path_str);
                if !fs.exists(&full_path) {
                    eprintln!(
                        "Warning: marketplace declares agent '{path_str}' but path does not exist"
                    );
                    continue;
                }
                if let Ok(content) = fs.read_to_string(&full_path)
                    && let Some(fm) = parse_agent_frontmatter_str(&content)
                {
                    let name = full_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    artifacts.push(Artifact {
                        kind: ArtifactKind::Agent,
                        name,
                        description: fm.description,
                        path: full_path,
                        version: fm.version,
                        deprecation: fm.deprecation,
                    });
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
                    eprintln!(
                        "Warning: marketplace declares skill '{path_str}' but path does not exist"
                    );
                    continue;
                }
                let skill_md = full_path.join("SKILL.md");
                if !fs.exists(&skill_md) {
                    eprintln!(
                        "Warning: marketplace declares skill '{path_str}' but SKILL.md is missing"
                    );
                    continue;
                }
                if let Ok(content) = fs.read_to_string(&skill_md)
                    && let Some(fm) = parse_frontmatter_str(&content)
                {
                    let name = full_path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    artifacts.push(Artifact {
                        kind: ArtifactKind::Skill,
                        name,
                        description: fm.description,
                        path: full_path,
                        version: fm.version,
                        deprecation: fm.deprecation,
                    });
                }
            }
        }
    }
}

fn walk_dir_with(dir: &Path, artifacts: &mut Vec<Artifact>, fs: &dyn Filesystem) -> Result<()> {
    let Ok(entries) = fs.read_dir(dir) else {
        return Ok(());
    };

    for entry in entries {
        let name_str = entry.file_name.clone();

        // Skip hidden directories
        if name_str.starts_with('.') {
            continue;
        }

        if entry.is_dir {
            let skill_md = entry.path.join("SKILL.md");
            if fs.exists(&skill_md)
                && let Ok(content) = fs.read_to_string(&skill_md)
                && let Some(fm) = parse_frontmatter_str(&content)
            {
                artifacts.push(Artifact {
                    kind: ArtifactKind::Skill,
                    name: name_str.clone(),
                    description: fm.description,
                    path: entry.path.clone(),
                    version: fm.version,
                    deprecation: fm.deprecation,
                });
            }
            walk_dir_with(&entry.path, artifacts, fs)?;
        } else if entry.path.extension().is_some_and(|ext| ext == "md")
            && name_str != "SKILL.md"
            && let Ok(content) = fs.read_to_string(&entry.path)
            && let Some(fm) = parse_agent_frontmatter_str(&content)
        {
            let agent_name = name_str.trim_end_matches(".md").to_string();
            artifacts.push(Artifact {
                kind: ArtifactKind::Agent,
                name: agent_name,
                description: fm.description,
                path: entry.path.clone(),
                version: fm.version,
                deprecation: fm.deprecation,
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pure frontmatter parsing (unchanged — no I/O)
// ---------------------------------------------------------------------------

struct Frontmatter {
    description: String,
    version: Option<String>,
    deprecation: Option<Deprecation>,
}

fn parse_deprecation(fm_text: &str) -> Option<Deprecation> {
    let deprecated = extract_field(fm_text, "deprecated")?;
    if deprecated != "true" {
        return None;
    }
    Some(Deprecation {
        reason: extract_field(fm_text, "deprecated_reason"),
        replacement: extract_field(fm_text, "deprecated_replacement"),
    })
}

fn parse_frontmatter_str(content: &str) -> Option<Frontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let fm_text = &rest[..end];
    Some(Frontmatter {
        description: extract_field(fm_text, "description").unwrap_or_default(),
        version: extract_field(fm_text, "version"),
        deprecation: parse_deprecation(fm_text),
    })
}

fn parse_agent_frontmatter_str(content: &str) -> Option<Frontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let fm_text = &rest[..end];

    let has_name = fm_text.lines().any(|l| l.starts_with("name:"));
    let has_desc = fm_text.lines().any(|l| l.starts_with("description:"));
    if !has_name || !has_desc {
        return None;
    }

    Some(Frontmatter {
        description: extract_field(fm_text, "description").unwrap_or_default(),
        version: extract_field(fm_text, "version"),
        deprecation: parse_deprecation(fm_text),
    })
}

fn extract_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter
        .lines()
        .find(|l| l.starts_with(&prefix))
        .map(|l| l[prefix.len()..].trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{agent_content, skill_content};
    use std::path::PathBuf;

    // ---------------------------------------------------------------------------
    // Pure parsing tests (unchanged)
    // ---------------------------------------------------------------------------

    // --- extract_field ---

    #[test]
    fn extract_field_basic() {
        let text = "name: my-agent\ndescription: A thing";
        assert_eq!(extract_field(text, "name"), Some("my-agent".to_string()));
    }

    #[test]
    fn extract_field_quoted_value() {
        let text = "name: \"my-agent\"";
        assert_eq!(extract_field(text, "name"), Some("my-agent".to_string()));
    }

    #[test]
    fn extract_field_not_present() {
        let text = "name: my-agent";
        assert_eq!(extract_field(text, "version"), None);
    }

    #[test]
    fn extract_field_empty_value_filtered() {
        let text = "name: ";
        assert_eq!(extract_field(text, "name"), None);
    }

    #[test]
    fn extract_field_extra_whitespace_trimmed() {
        let text = "name:   spaced-value   ";
        assert_eq!(extract_field(text, "name"), Some("spaced-value".to_string()));
    }

    #[test]
    fn extract_field_multiple_fields_picks_correct_one() {
        let text = "name: my-agent\ndescription: A thing\nversion: 1.0.0";
        assert_eq!(extract_field(text, "description"), Some("A thing".to_string()));
    }

    #[test]
    fn extract_field_no_prefix_collision() {
        // key "name" must not match line "namespace: foo"
        let text = "namespace: foo";
        assert_eq!(extract_field(text, "name"), None);
    }

    // --- parse_deprecation ---

    #[test]
    fn parse_deprecation_true_with_reason_and_replacement() {
        let text =
            "deprecated: true\ndeprecated_reason: Too old\ndeprecated_replacement: new-agent";
        let dep = parse_deprecation(text).expect("expected Some");
        assert_eq!(dep.reason.as_deref(), Some("Too old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-agent"));
    }

    #[test]
    fn parse_deprecation_true_no_reason_or_replacement() {
        let text = "deprecated: true";
        let dep = parse_deprecation(text).expect("expected Some");
        assert!(dep.reason.is_none());
        assert!(dep.replacement.is_none());
    }

    #[test]
    fn parse_deprecation_false_returns_none() {
        let text = "deprecated: false";
        assert!(parse_deprecation(text).is_none());
    }

    #[test]
    fn parse_deprecation_absent_returns_none() {
        let text = "name: my-agent\ndescription: A thing";
        assert!(parse_deprecation(text).is_none());
    }

    // --- parse_frontmatter_str ---

    #[test]
    fn parse_frontmatter_str_valid_all_fields() {
        let content = "---\ndescription: Test skill\nversion: 1.2.3\n---\n# content";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "Test skill");
        assert_eq!(fm.version.as_deref(), Some("1.2.3"));
        assert!(fm.deprecation.is_none());
    }

    #[test]
    fn parse_frontmatter_str_no_delimiters_returns_none() {
        let content = "description: Test skill\n# content";
        assert!(parse_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_frontmatter_str_missing_closing_delimiter_returns_none() {
        let content = "---\ndescription: Test skill\n# content";
        // "---\n" then rest="description: Test skill\n# content", no "---" found
        assert!(parse_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_frontmatter_str_without_version() {
        let content = "---\ndescription: No version here\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "No version here");
        assert!(fm.version.is_none());
    }

    #[test]
    fn parse_frontmatter_str_with_deprecation() {
        let content =
            "---\ndescription: Old skill\ndeprecated: true\ndeprecated_reason: Replaced\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        let dep = fm.deprecation.expect("expected deprecation");
        assert_eq!(dep.reason.as_deref(), Some("Replaced"));
    }

    // --- parse_agent_frontmatter_str ---

    #[test]
    fn parse_agent_frontmatter_str_valid() {
        let content = "---\nname: my-agent\ndescription: Does things\n---\n# body";
        let fm = parse_agent_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "Does things");
    }

    #[test]
    fn parse_agent_frontmatter_str_missing_name_returns_none() {
        let content = "---\ndescription: Does things\n---\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_agent_frontmatter_str_missing_description_returns_none() {
        let content = "---\nname: my-agent\n---\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_agent_frontmatter_str_no_delimiters_returns_none() {
        let content = "name: my-agent\ndescription: Does things\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    // ---------------------------------------------------------------------------
    // scan_source_with tests using FakeFilesystem
    // ---------------------------------------------------------------------------

    #[test]
    fn scan_empty_directory_returns_empty() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_ignores_md_file_without_any_frontmatter() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/plain.md", "# No frontmatter here");
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_ignores_md_file_without_agent_frontmatter() {
        let fs = FakeFilesystem::new();
        // Has frontmatter but no 'name:' field — not an agent
        fs.add_file("/repo/not-agent.md", "---\ndescription: only desc\n---\n");
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_finds_agent_with_valid_frontmatter() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/my-agent.md", agent_content("my-agent", "Does things"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "my-agent");
        assert_eq!(result[0].kind, ArtifactKind::Agent);
        assert_eq!(result[0].description, "Does things");
        assert_eq!(result[0].path, PathBuf::from("/repo/my-agent.md"));
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let fs = FakeFilesystem::new();
        // File inside a hidden dir — should be ignored
        fs.add_file("/repo/.hidden/secret.md", agent_content("secret", "Hidden"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_finds_multiple_agents_sorted_by_name() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/zebra.md", agent_content("zebra", "Z agent"));
        fs.add_file("/repo/alpha.md", agent_content("alpha", "A agent"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "alpha");
        assert_eq!(result[1].name, "zebra");
    }

    #[test]
    fn scan_finds_skill_with_skill_md() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/my-skill/SKILL.md", skill_content("A skill"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "my-skill");
        assert_eq!(result[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn scan_finds_both_agents_and_skills() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/alpha.md", agent_content("alpha", "An agent"));
        fs.add_file("/repo/my-skill/SKILL.md", skill_content("A skill"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 2);
        let kinds: Vec<_> = result.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&ArtifactKind::Agent));
        assert!(kinds.contains(&ArtifactKind::Skill));
    }

    // ---------------------------------------------------------------------------
    // Marketplace: source-path fallback (no explicit agents/skills arrays)
    // ---------------------------------------------------------------------------

    fn marketplace_json(plugins_json: &str) -> String {
        format!(
            r#"{{
  "name": "test-marketplace",
  "owner": {{ "name": "Test" }},
  "plugins": [{plugins_json}]
}}"#
        )
    }

    #[test]
    fn marketplace_source_path_walks_directory_for_agents() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/reviewer.md",
            agent_content("reviewer", "Reviews code"),
        );
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "reviewer");
        assert_eq!(result[0].kind, ArtifactKind::Agent);
    }

    #[test]
    fn marketplace_source_path_walks_directory_for_skills() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/my-skill/SKILL.md",
            skill_content("A discovered skill"),
        );
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "my-skill");
        assert_eq!(result[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn marketplace_source_path_finds_mixed_artifacts() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/checker.md",
            agent_content("checker", "Checks things"),
        );
        fs.add_file("/repo/plugins/my-plugin/pdf/SKILL.md", skill_content("PDF processing"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<_> = result.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"checker"));
        assert!(names.contains(&"pdf"));
    }

    #[test]
    fn marketplace_explicit_arrays_take_precedence_over_source_walk() {
        let fs = FakeFilesystem::new();
        // Plugin has both source AND explicit skills array — should use explicit only
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{
                    "name": "doc-skills",
                    "source": "./",
                    "strict": false,
                    "skills": ["./skills/pdf"]
                }"#,
            ),
        );
        fs.add_file("/repo/skills/pdf/SKILL.md", skill_content("PDF skill"));
        // This agent exists in the repo but isn't in the explicit arrays
        fs.add_file("/repo/extra-agent.md", agent_content("extra", "Should not appear"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "pdf");
        assert_eq!(result[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn marketplace_multiple_source_plugins_all_walked() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{ "name": "plugin-a", "source": "./plugins/a" },
                   { "name": "plugin-b", "source": "./plugins/b" }"#,
            ),
        );
        fs.add_file("/repo/plugins/a/agent-a.md", agent_content("agent-a", "From plugin A"));
        fs.add_file("/repo/plugins/b/agent-b.md", agent_content("agent-b", "From plugin B"));
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<_> = result.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"agent-a"));
        assert!(names.contains(&"agent-b"));
    }

    #[test]
    fn marketplace_missing_source_path_warns_and_continues() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "ghost", "source": "./nonexistent" }"#),
        );
        // Should not error, just warn and return empty
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn marketplace_object_source_warns_and_continues() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{ "name": "remote-plugin", "source": { "source": "url", "url": "https://github.com/example/plugin.git" } }"#,
            ),
        );
        // Remote sources are not supported — should warn and return empty
        let result = scan_source_with(Path::new("/repo"), &fs).unwrap();
        assert!(result.is_empty());
    }
}
