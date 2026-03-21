use anyhow::{Result, bail};
use std::path::Path;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::source;
use crate::source_iter;
use crate::types::ArtifactKind;

pub fn info_with(name: &str, ctx: &AppContext<'_>) -> Result<()> {
    // Search both scopes and both kinds
    for local in [false, true] {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            let dir = ctx.paths.install_dir(kind, local);
            let path = kind.installed_path(name, &dir);
            if ctx.fs.exists(&path) {
                return show_info_with(name, kind, local, &path, ctx);
            }
        }
    }

    bail!("No installed artifact named '{name}' found.");
}

fn show_info_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    path: &Path,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let scope = if local { "local" } else { "global" };
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
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
        let current_checksum = checksum::checksum_artifact_with(path, kind, ctx.fs)?;
        if current_checksum != entry.installed_checksum {
            println!("Disk SHA:    {current_checksum}  (locally modified)");
        }
    } else {
        println!("Lock entry:  (none — untracked)");
    }

    // Check source for deprecation and available version
    source::auto_update_all_with(ctx).ok();
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
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
    if kind == ArtifactKind::Skill && ctx.fs.is_dir(path) {
        println!();
        println!("Files:");
        list_skill_files_with(path, "  ", ctx)?;
    }

    Ok(())
}

fn list_skill_files_with(dir: &Path, indent: &str, ctx: &AppContext<'_>) -> Result<()> {
    let mut entries = ctx.fs.read_dir(dir)?;
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    for entry in entries {
        let name_str = &entry.file_name;
        if name_str.starts_with('.') {
            continue;
        }

        if entry.is_dir {
            println!("{indent}{name_str}/");
            list_skill_files_with(&entry.path, &format!("{indent}  "), ctx)?;
        } else {
            println!("{indent}{name_str}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy free-function API
// ---------------------------------------------------------------------------

pub fn info(name: &str) -> Result<()> {
    let paths = ConfigPaths::from_env()?;
    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };
    info_with(name, &ctx)
}
