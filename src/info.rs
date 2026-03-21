use anyhow::{Result, bail};
use std::fs;
use std::path::Path;

use crate::checksum;
use crate::config;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::ArtifactKind;

pub fn info(name: &str) -> Result<()> {
    // Search both scopes and both kinds
    for local in [false, true] {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            let dir = config::install_dir(kind, local)?;
            let path = kind.installed_path(name, &dir);
            if path.exists() {
                return show_info(name, kind, local, &path);
            }
        }
    }

    bail!("No installed artifact named '{name}' found.");
}

fn show_info(name: &str, kind: ArtifactKind, local: bool, path: &Path) -> Result<()> {
    let scope = if local { "local" } else { "global" };
    let lock = lockfile::load(local)?;
    let lock_entry = lock.packages.get(name);

    println!("Name:        {name}");
    println!("Type:        {kind}");
    println!("Scope:       {scope}");
    println!("Path:        {}", path.display());

    if let Some(entry) = lock_entry {
        if let Some(v) = &entry.version {
            println!("Version:     {v}");
        }
        println!("Installed:   {}", entry.installed_at);
        println!("Source:      {} ({})", entry.source.repo, entry.source.path);
        println!("Source SHA:  {}", entry.source_checksum);
        println!("Install SHA: {}", entry.installed_checksum);

        // Check for local modifications
        let current_checksum = checksum::checksum_artifact(path, kind)?;
        if current_checksum != entry.installed_checksum {
            println!("Disk SHA:    {current_checksum}  (locally modified)");
        }
    } else {
        println!("Lock entry:  (none — untracked)");
    }

    // Check source for deprecation and available version
    source::auto_update_all().ok();
    let sources = config::load_sources()?;
    for sa in source_iter::each_source_artifact(&sources.sources) {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            if let Some(dep) = &sa.artifact.deprecation {
                println!("Status:      DEPRECATED");
                if let Some(reason) = &dep.reason {
                    println!("  Reason:    {reason}");
                }
                if let Some(repl) = &dep.replacement {
                    println!("  Replace:   {repl}");
                }
            }
            if let Some(v) = sa.artifact.version.as_deref() {
                let installed_v = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-");
                if v != installed_v {
                    println!("Available:   v{v} (update available)");
                }
            }
        }
    }

    // For skills: list files
    if kind == ArtifactKind::Skill && path.is_dir() {
        println!();
        println!("Files:");
        list_skill_files(path, "  ")?;
    }

    Ok(())
}

fn list_skill_files(dir: &Path, indent: &str) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        if entry.path().is_dir() {
            println!("{indent}{name_str}/");
            list_skill_files(&entry.path(), &format!("{indent}  "))?;
        } else {
            println!("{indent}{name_str}");
        }
    }
    Ok(())
}
