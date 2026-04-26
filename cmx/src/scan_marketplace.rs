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
