use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::types::ArtifactKind;

pub fn list_kind(kind: ArtifactKind) -> Result<()> {
    let global = installed_names(kind, false)?;
    let local = installed_names(kind, true)?;

    if global.is_empty() && local.is_empty() {
        println!("No {kind}s installed.");
        return Ok(());
    }

    if !global.is_empty() {
        println!("Global {kind}s:");
        for name in &global {
            println!("  {name}");
        }
    }

    if !local.is_empty() {
        if !global.is_empty() {
            println!();
        }
        println!("Local {kind}s:");
        for name in &local {
            println!("  {name}");
        }
    }

    Ok(())
}

pub fn list_all() -> Result<()> {
    let global_agents = installed_names(ArtifactKind::Agent, false)?;
    let local_agents = installed_names(ArtifactKind::Agent, true)?;
    let global_skills = installed_names(ArtifactKind::Skill, false)?;
    let local_skills = installed_names(ArtifactKind::Skill, true)?;

    if global_agents.is_empty()
        && local_agents.is_empty()
        && global_skills.is_empty()
        && local_skills.is_empty()
    {
        println!("Nothing installed.");
        return Ok(());
    }

    print_section("Global agents", &global_agents);
    print_section("Local agents", &local_agents);
    print_section("Global skills", &global_skills);
    print_section("Local skills", &local_skills);

    Ok(())
}

fn print_section(label: &str, names: &[String]) {
    println!("{label}:");
    if names.is_empty() {
        println!("  (none)");
    } else {
        for name in names {
            println!("  {name}");
        }
    }
    println!();
}

fn installed_names(kind: ArtifactKind, local: bool) -> Result<Vec<String>> {
    let dir = install_dir(kind, local)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("Failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        match kind {
            ArtifactKind::Agent => {
                if name_str.ends_with(".md") {
                    names.push(name_str.trim_end_matches(".md").to_string());
                }
            }
            ArtifactKind::Skill => {
                if entry.path().is_dir() {
                    names.push(name_str.into_owned());
                }
            }
        }
    }

    names.sort();
    Ok(names)
}

fn install_dir(kind: ArtifactKind, local: bool) -> Result<PathBuf> {
    let subdir = match kind {
        ArtifactKind::Agent => "agents",
        ArtifactKind::Skill => "skills",
    };

    if local {
        Ok(PathBuf::from(".claude").join(subdir))
    } else {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".claude").join(subdir))
    }
}
