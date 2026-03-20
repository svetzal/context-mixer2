use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::config;
use crate::scan;
use crate::types::{Artifact, SourceEntry, SourceType};

pub fn add(name: &str, path_or_url: &str) -> Result<()> {
    let mut sources = config::load_sources()?;

    if sources.sources.contains_key(name) {
        bail!("Source '{}' already exists. Remove it first to re-register.", name);
    }

    let entry = if looks_like_url(path_or_url) {
        add_git_source(name, path_or_url)?
    } else {
        add_local_source(path_or_url)?
    };

    let local_path = config::resolve_local_path(&entry);
    let artifacts = scan::scan_source(&local_path)?;
    let (agents, skills) = count_artifacts(&artifacts);

    sources.sources.insert(name.to_string(), entry);
    config::save_sources(&sources)?;

    println!("Source '{name}' registered: {agents} agent(s), {skills} skill(s) found.");
    Ok(())
}

pub fn list() -> Result<()> {
    let sources = config::load_sources()?;

    if sources.sources.is_empty() {
        println!("No sources registered.");
        println!();
        println!("Add one with: cmx source add <name> <path-or-url>");
        return Ok(());
    }

    for (name, entry) in &sources.sources {
        let location = match entry.source_type {
            SourceType::Local => entry.path.as_ref().map(|p| p.display().to_string()),
            SourceType::Git => entry.url.clone(),
        };
        let kind = match entry.source_type {
            SourceType::Local => "local",
            SourceType::Git => "git",
        };
        println!(
            "  {name:<28} ({kind}) {loc}",
            loc = location.unwrap_or_default()
        );
    }

    Ok(())
}

pub fn browse(name: &str) -> Result<()> {
    let sources = config::load_sources()?;

    let entry = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found. Run 'cmx source list' to see registered sources."))?;

    let local_path = config::resolve_local_path(entry);
    if !local_path.exists() {
        bail!(
            "Source path {} does not exist. {}",
            local_path.display(),
            match entry.source_type {
                SourceType::Git => "Try 'cmx source pull' to fetch it.",
                SourceType::Local => "Check that the directory still exists.",
            }
        );
    }

    let artifacts = scan::scan_source(&local_path)?;

    if artifacts.is_empty() {
        println!("No agents or skills found in '{name}'.");
        return Ok(());
    }

    let agents: Vec<_> = artifacts.iter().filter(|a| a.kind() == "agent").collect();
    let skills: Vec<_> = artifacts.iter().filter(|a| a.kind() == "skill").collect();

    if !agents.is_empty() {
        println!("Agents:");
        for a in &agents {
            println!("  {}", a.name());
        }
    }

    if !skills.is_empty() {
        if !agents.is_empty() {
            println!();
        }
        println!("Skills:");
        for s in &skills {
            println!("  {}", s.name());
        }
    }

    Ok(())
}

pub fn pull(name: &str) -> Result<()> {
    let sources = config::load_sources()?;

    let entry = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found."))?;

    match entry.source_type {
        SourceType::Local => {
            println!("Source '{name}' is local — nothing to pull.");
            return Ok(());
        }
        SourceType::Git => {}
    }

    let clone_path = entry
        .local_clone
        .as_ref()
        .context("Git source has no local clone path")?;

    if !clone_path.exists() {
        bail!(
            "Clone directory {} does not exist. Try removing and re-adding the source.",
            clone_path.display()
        );
    }

    println!("Pulling latest for '{name}'...");

    let output = Command::new("git")
        .args(["-C", &clone_path.display().to_string(), "pull"])
        .output()
        .context("Failed to run git pull")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git pull failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");

    let artifacts = scan::scan_source(clone_path)?;
    let (agents, skills) = count_artifacts(&artifacts);
    println!("Source '{name}': {agents} agent(s), {skills} skill(s) found.");

    Ok(())
}

pub fn remove(name: &str) -> Result<()> {
    let mut sources = config::load_sources()?;

    let entry = sources
        .sources
        .remove(name)
        .with_context(|| format!("Source '{name}' not found."))?;

    config::save_sources(&sources)?;

    if let Some(clone_path) = &entry.local_clone {
        if clone_path.exists() {
            std::fs::remove_dir_all(clone_path).with_context(|| {
                format!("Failed to remove cloned repo at {}", clone_path.display())
            })?;
            println!("Source '{name}' removed (cloned repo deleted).");
        } else {
            println!("Source '{name}' removed.");
        }
    } else {
        println!("Source '{name}' removed.");
    }

    Ok(())
}

fn add_local_source(path_str: &str) -> Result<SourceEntry> {
    let path = PathBuf::from(path_str);
    let path = path
        .canonicalize()
        .with_context(|| format!("Path '{}' does not exist or is not accessible.", path_str))?;

    if !path.is_dir() {
        bail!("'{}' is not a directory.", path.display());
    }

    Ok(SourceEntry {
        source_type: SourceType::Local,
        path: Some(path),
        url: None,
        local_clone: None,
        branch: None,
    })
}

fn add_git_source(name: &str, url: &str) -> Result<SourceEntry> {
    let clone_dir = config::git_clones_dir()?.join(name);

    if clone_dir.exists() {
        bail!(
            "Clone directory {} already exists. Remove it or choose a different name.",
            clone_dir.display()
        );
    }

    println!("Cloning {url} to {}...", clone_dir.display());

    let output = Command::new("git")
        .args(["clone", url, &clone_dir.display().to_string()])
        .output()
        .context("Failed to run git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {stderr}");
    }

    Ok(SourceEntry {
        source_type: SourceType::Git,
        path: None,
        url: Some(url.to_string()),
        local_clone: Some(clone_dir),
        branch: Some("main".to_string()),
    })
}

fn count_artifacts(artifacts: &[Artifact]) -> (usize, usize) {
    let agents = artifacts.iter().filter(|a| a.kind() == "agent").count();
    let skills = artifacts.iter().filter(|a| a.kind() == "skill").count();
    (agents, skills)
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
}
