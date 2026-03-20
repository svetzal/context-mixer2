use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::types::Artifact;

pub fn scan_source(root: &Path) -> Result<Vec<Artifact>> {
    let mut artifacts = Vec::new();
    walk_dir(root, &mut artifacts)?;
    artifacts.sort_by(|a, b| a.name().cmp(b.name()));
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
            // Check for skill: directory containing SKILL.md with frontmatter
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() && has_frontmatter(&skill_md) {
                artifacts.push(Artifact::Skill {
                    name: name_str.into_owned(),
                    path: path.clone(),
                });
            }
            // Recurse regardless — skills can be nested
            walk_dir(&path, artifacts)?;
        } else if path.extension().is_some_and(|ext| ext == "md") && name_str != "SKILL.md" {
            // Check for agent: .md file with frontmatter containing name and description
            if has_agent_frontmatter(&path) {
                let agent_name = name_str.trim_end_matches(".md").to_string();
                artifacts.push(Artifact::Agent {
                    name: agent_name,
                    path: path.clone(),
                });
            }
        }
    }

    Ok(())
}

fn has_frontmatter(path: &Path) -> bool {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    content.starts_with("---")
}

fn has_agent_frontmatter(path: &Path) -> bool {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    if !content.starts_with("---") {
        return false;
    }

    // Find the closing --- of frontmatter
    let rest = &content[3..];
    let end = match rest.find("---") {
        Some(pos) => pos,
        None => return false,
    };

    let frontmatter = &rest[..end];
    let has_name = frontmatter.lines().any(|l| l.starts_with("name:"));
    let has_desc = frontmatter.lines().any(|l| l.starts_with("description:"));
    has_name && has_desc
}
