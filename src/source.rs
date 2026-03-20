use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use std::process::Command;

use crate::config;
use crate::scan;
use crate::types::{Artifact, SourceEntry, SourceType};

const AUTO_UPDATE_MINUTES: i64 = 60;

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
    auto_update_source(name)?;

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
                SourceType::Git => "Try 'cmx source update' to fetch it.",
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
            let v = a.version().map(|v| format!("  v{v}")).unwrap_or_default();
            let dep = format_deprecation(a);
            println!("  {}{v}{dep}", a.name());
        }
    }

    if !skills.is_empty() {
        if !agents.is_empty() {
            println!();
        }
        println!("Skills:");
        for s in &skills {
            let v = s.version().map(|v| format!("  v{v}")).unwrap_or_default();
            let dep = format_deprecation(s);
            println!("  {}{v}{dep}", s.name());
        }
    }

    Ok(())
}

pub fn update(name: Option<&str>) -> Result<()> {
    let sources = config::load_sources()?;

    match name {
        Some(n) => {
            if !sources.sources.contains_key(n) {
                bail!("Source '{n}' not found.");
            }
            pull_source(n)?;
        }
        None => {
            let git_sources: Vec<_> = sources
                .sources
                .iter()
                .filter(|(_, e)| matches!(e.source_type, SourceType::Git))
                .map(|(n, _)| n.clone())
                .collect();

            if git_sources.is_empty() {
                println!("No git-backed sources to update.");
                return Ok(());
            }

            for source_name in &git_sources {
                pull_source(source_name)?;
            }
        }
    }

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

/// Auto-update a git source if it hasn't been updated recently.
pub fn auto_update_source(name: &str) -> Result<()> {
    let sources = config::load_sources()?;
    let entry = match sources.sources.get(name) {
        Some(e) => e,
        None => return Ok(()),
    };

    if !matches!(entry.source_type, SourceType::Git) {
        return Ok(());
    }

    if is_stale(entry) {
        pull_source(name)?;
    }

    Ok(())
}

/// Auto-update all stale git sources.
pub fn auto_update_all() -> Result<()> {
    let sources = config::load_sources()?;
    for (name, entry) in &sources.sources {
        if matches!(entry.source_type, SourceType::Git) && is_stale(entry) {
            pull_source(name)?;
        }
    }
    Ok(())
}

fn is_stale(entry: &SourceEntry) -> bool {
    let Some(last) = &entry.last_updated else {
        return true;
    };
    let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    let age = Utc::now().signed_duration_since(last_time);
    age.num_minutes() >= AUTO_UPDATE_MINUTES
}

fn pull_source(name: &str) -> Result<()> {
    let mut sources = config::load_sources()?;
    let entry = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found."))?;

    match entry.source_type {
        SourceType::Local => {
            // Update timestamp for local sources too
            let mut entry = entry.clone();
            entry.last_updated = Some(Utc::now().to_rfc3339());
            sources.sources.insert(name.to_string(), entry);
            config::save_sources(&sources)?;
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

    println!("Updating '{name}'...");

    let output = Command::new("git")
        .args(["-C", &clone_path.display().to_string(), "pull", "--quiet"])
        .output()
        .context("Failed to run git pull")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git pull failed: {stderr}");
    }

    // Update timestamp
    let mut entry = entry.clone();
    entry.last_updated = Some(Utc::now().to_rfc3339());
    sources.sources.insert(name.to_string(), entry);
    config::save_sources(&sources)?;

    let local_path = config::resolve_local_path(
        sources.sources.get(name).unwrap(),
    );
    let artifacts = scan::scan_source(&local_path)?;
    let (agents, skills) = count_artifacts(&artifacts);
    println!("Source '{name}': {agents} agent(s), {skills} skill(s).");

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
        last_updated: Some(Utc::now().to_rfc3339()),
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
        last_updated: Some(Utc::now().to_rfc3339()),
    })
}

fn count_artifacts(artifacts: &[Artifact]) -> (usize, usize) {
    let agents = artifacts.iter().filter(|a| a.kind() == "agent").count();
    let skills = artifacts.iter().filter(|a| a.kind() == "skill").count();
    (agents, skills)
}

fn format_deprecation(artifact: &Artifact) -> String {
    let Some(dep) = artifact.deprecation() else {
        return String::new();
    };

    let mut parts = vec!["  ⛔ DEPRECATED".to_string()];

    if let Some(reason) = &dep.reason {
        parts.push(format!(": {reason}"));
    }

    if let Some(replacement) = &dep.replacement {
        parts.push(format!(" (use {replacement} instead)"));
    }

    parts.join("")
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
}
