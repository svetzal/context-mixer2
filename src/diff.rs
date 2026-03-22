use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::ArtifactKind;

pub async fn diff_with(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    source::auto_update_all_with(ctx)?;

    // Find the installed file on disk (global then local)
    let (installed_path, local) = find_installed_on_disk_with(name, kind, ctx)?;

    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;

    // Compare checksums
    let installed_checksum = checksum::checksum_artifact_with(&installed_path, kind, ctx.fs)?;
    let source_checksum = checksum::checksum_artifact_with(&source_path, kind, ctx.fs)?;

    if installed_checksum == source_checksum {
        println!("{name} is up to date with source.");
        return Ok(());
    }

    // Get installed version from lock file if available
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let installed_version = lock
        .packages
        .get(name)
        .and_then(|e| e.version.as_deref())
        .unwrap_or("unversioned");

    let source_ver_display = source_version.as_deref().unwrap_or("unversioned");
    let scope = if local { "local" } else { "global" };

    println!("Comparing {name} ({kind})");
    println!("  Installed ({scope}): {installed_version}");
    println!("  Source ({source_name}): {source_ver_display}");
    println!();

    // Build diff text
    let diff_text = match kind {
        ArtifactKind::Agent => diff_files_with(&installed_path, &source_path, ctx)?,
        ArtifactKind::Skill => diff_dirs_with(&installed_path, &source_path, ctx)?,
    };

    println!("Analyzing differences...");
    println!();

    let system_prompt = "You are a technical analyst comparing two versions of an AI coding assistant artifact (an agent definition or skill definition written in markdown). \
        Provide a clear, concise summary of the differences. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed\n\
        2. Whether the update is significant or cosmetic\n\
        3. A recommendation: should the user update their installed version?\n\n\
        Keep your analysis brief and actionable — a few paragraphs at most.";

    let user_prompt = format!(
        "Compare these two versions of the {kind} '{name}':\n\
        - Installed version: {installed_version}\n\
        - Source version: {source_ver_display}\n\n\
        {diff_text}"
    );

    let analysis = match ctx.llm {
        Some(llm) => llm.analyze(system_prompt, &user_prompt).await?,
        None => bail!("LLM client not configured for diff analysis"),
    };
    println!("{analysis}");

    Ok(())
}

fn find_installed_on_disk_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, bool)> {
    for local in [false, true] {
        let dir = ctx.paths.install_dir(kind, local);
        let path = kind.installed_path(name, &dir);
        if ctx.fs.exists(&path) {
            return Ok((path, local));
        }
    }

    bail!("No installed {kind} named '{name}' found on disk.");
}

fn find_in_sources_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, String, Option<String>)> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            return Ok((sa.artifact.path, sa.source_name, sa.artifact.version));
        }
    }

    bail!("No {kind} named '{name}' found in any registered source.");
}

fn diff_files_with(installed: &Path, source: &Path, ctx: &AppContext<'_>) -> Result<String> {
    let installed_content = ctx
        .fs
        .read_to_string(installed)
        .with_context(|| format!("Failed to read {}", installed.display()))?;
    let source_content = ctx
        .fs
        .read_to_string(source)
        .with_context(|| format!("Failed to read {}", source.display()))?;

    Ok(format!(
        "=== INSTALLED VERSION ===\n{installed_content}\n\n=== SOURCE VERSION ===\n{source_content}"
    ))
}

fn diff_dirs_with(installed: &Path, source: &Path, ctx: &AppContext<'_>) -> Result<String> {
    let mut result = String::new();

    let installed_files = collect_relative_files_with(installed, ctx)?;
    let source_files = collect_relative_files_with(source, ctx)?;

    for f in &installed_files {
        if !source_files.contains(f) {
            let _ = writeln!(result, "--- Only in installed: {f}");
        }
    }

    for f in &source_files {
        if !installed_files.contains(f) {
            let _ = writeln!(result, "+++ Only in source: {f}");
        }
    }

    for f in &installed_files {
        if source_files.contains(f) {
            let i_path = installed.join(f);
            let s_path = source.join(f);
            let i_content = ctx.fs.read_to_string(&i_path).unwrap_or_default();
            let s_content = ctx.fs.read_to_string(&s_path).unwrap_or_default();
            if i_content != s_content {
                let _ = write!(
                    result,
                    "\n=== {f} (INSTALLED) ===\n{i_content}\n=== {f} (SOURCE) ===\n{s_content}\n"
                );
            }
        }
    }

    Ok(result)
}

fn collect_relative_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<String>> {
    let mut files = collect_files_with(dir, ctx)?
        .into_iter()
        .map(|p| p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().to_string())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn collect_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = ctx.fs.read_dir(dir)?;
    for entry in entries {
        if entry.file_name.starts_with('.') {
            continue;
        }
        if entry.is_dir {
            files.extend(collect_files_with(&entry.path, ctx)?);
        } else {
            files.push(entry.path);
        }
    }
    Ok(files)
}
