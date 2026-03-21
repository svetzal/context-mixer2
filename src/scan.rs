use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::types::{Artifact, Deprecation};

pub fn scan_source(root: &Path) -> Result<Vec<Artifact>> {
    let marketplace = root.join(".claude-plugin").join("marketplace.json");

    let mut artifacts = if marketplace.exists() {
        scan_marketplace(root, &marketplace)?
    } else {
        let mut arts = Vec::new();
        walk_dir(root, &mut arts)?;
        arts
    };

    artifacts.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(artifacts)
}

fn scan_marketplace(root: &Path, marketplace_path: &Path) -> Result<Vec<Artifact>> {
    let content = fs::read_to_string(marketplace_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let mut artifacts = Vec::new();

    if let Some(plugins) = manifest.get("plugins").and_then(|p| p.as_array()) {
        for plugin in plugins {
            // Scan declared agents
            if let Some(agents) = plugin.get("agents").and_then(|a| a.as_array()) {
                for agent_path in agents {
                    if let Some(path_str) = agent_path.as_str() {
                        let full_path = root.join(path_str);
                        if !full_path.exists() {
                            eprintln!(
                                "Warning: marketplace declares agent '{}' but path does not exist",
                                path_str
                            );
                            continue;
                        }
                        if let Some(fm) = parse_agent_frontmatter(&full_path) {
                            let name = full_path
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            artifacts.push(Artifact::Agent {
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
                        if !full_path.exists() {
                            eprintln!(
                                "Warning: marketplace declares skill '{}' but path does not exist",
                                path_str
                            );
                            continue;
                        }
                        let skill_md = full_path.join("SKILL.md");
                        if !skill_md.exists() {
                            eprintln!(
                                "Warning: marketplace declares skill '{}' but SKILL.md is missing",
                                path_str
                            );
                            continue;
                        }
                        if let Some(fm) = parse_frontmatter(&skill_md) {
                            let name = full_path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            artifacts.push(Artifact::Skill {
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
    }

    Ok(artifacts)
}

fn walk_dir(dir: &Path, artifacts: &mut Vec<Artifact>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden directories
        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists()
                && let Some(fm) = parse_frontmatter(&skill_md)
            {
                artifacts.push(Artifact::Skill {
                    name: name_str.into_owned(),
                    description: fm.description,
                    path: path.clone(),
                    version: fm.version,
                    deprecation: fm.deprecation,
                });
            }
            walk_dir(&path, artifacts)?;
        } else if path.extension().is_some_and(|ext| ext == "md")
            && name_str != "SKILL.md"
            && let Some(fm) = parse_agent_frontmatter(&path)
        {
            let agent_name = name_str.trim_end_matches(".md").to_string();
            artifacts.push(Artifact::Agent {
                name: agent_name,
                description: fm.description,
                path: path.clone(),
                version: fm.version,
                deprecation: fm.deprecation,
            });
        }
    }

    Ok(())
}

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

fn parse_frontmatter(path: &Path) -> Option<Frontmatter> {
    let content = fs::read_to_string(path).ok()?;
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

fn parse_agent_frontmatter(path: &Path) -> Option<Frontmatter> {
    let content = fs::read_to_string(path).ok()?;
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
