use anyhow::{Context, Result, bail};
use mojentic::llm::gateways::{OllamaGateway, OpenAIGateway};
use mojentic::llm::{LlmBroker, LlmGateway, LlmMessage};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::checksum;
use crate::config;
use crate::fs_util;
use crate::lockfile;
use crate::scan;
use crate::source;
use crate::types::{ArtifactKind, LlmGatewayType};

pub async fn diff(name: &str, kind: ArtifactKind) -> Result<()> {
    source::auto_update_all()?;

    // Find the installed file on disk (global then local)
    let (installed_path, local) = find_installed_on_disk(name, kind)?;

    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources(name, kind)?;

    // Compare checksums
    let installed_checksum = checksum::checksum_artifact(&installed_path, kind)?;
    let source_checksum = checksum::checksum_artifact(&source_path, kind)?;

    if installed_checksum == source_checksum {
        println!("{name} is up to date with source.");
        return Ok(());
    }

    // Get installed version from lock file if available
    let lock = lockfile::load(local)?;
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
        ArtifactKind::Agent => diff_files(&installed_path, &source_path)?,
        ArtifactKind::Skill => diff_dirs(&installed_path, &source_path)?,
    };

    println!("Analyzing differences...");
    println!();

    let analysis =
        analyze_diff(name, kind, installed_version, source_ver_display, &diff_text).await?;
    println!("{analysis}");

    Ok(())
}

fn find_installed_on_disk(name: &str, kind: ArtifactKind) -> Result<(PathBuf, bool)> {
    for local in [false, true] {
        let dir = config::install_dir(kind, local)?;
        let path = kind.installed_path(name, &dir);
        if path.exists() {
            return Ok((path, local));
        }
    }

    bail!("No installed {kind} named '{name}' found on disk.");
}

fn find_in_sources(name: &str, kind: ArtifactKind) -> Result<(PathBuf, String, Option<String>)> {
    let sources = config::load_sources()?;

    for (source_name, entry) in &sources.sources {
        let source_root = config::resolve_local_path(entry);
        if !source_root.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&source_root) {
            for artifact in &artifacts {
                if artifact.name == name && artifact.kind == kind {
                    return Ok((
                        artifact.path.clone(),
                        source_name.clone(),
                        artifact.version.clone(),
                    ));
                }
            }
        }
    }

    bail!("No {kind} named '{name}' found in any registered source.");
}

fn diff_files(installed: &Path, source: &Path) -> Result<String> {
    let installed_content = fs::read_to_string(installed)
        .with_context(|| format!("Failed to read {}", installed.display()))?;
    let source_content = fs::read_to_string(source)
        .with_context(|| format!("Failed to read {}", source.display()))?;

    Ok(format!(
        "=== INSTALLED VERSION ===\n{installed_content}\n\n=== SOURCE VERSION ===\n{source_content}"
    ))
}

fn diff_dirs(installed: &Path, source: &Path) -> Result<String> {
    let mut result = String::new();

    let installed_files = collect_relative_files(installed)?;
    let source_files = collect_relative_files(source)?;

    for f in &installed_files {
        if !source_files.contains(f) {
            result.push_str(&format!("--- Only in installed: {f}\n"));
        }
    }

    for f in &source_files {
        if !installed_files.contains(f) {
            result.push_str(&format!("+++ Only in source: {f}\n"));
        }
    }

    for f in &installed_files {
        if source_files.contains(f) {
            let i_path = installed.join(f);
            let s_path = source.join(f);
            let i_content = fs::read_to_string(&i_path).unwrap_or_default();
            let s_content = fs::read_to_string(&s_path).unwrap_or_default();
            if i_content != s_content {
                result.push_str(&format!(
                    "\n=== {f} (INSTALLED) ===\n{i_content}\n=== {f} (SOURCE) ===\n{s_content}\n"
                ));
            }
        }
    }

    Ok(result)
}

fn collect_relative_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = fs_util::collect_files(dir)?
        .into_iter()
        .map(|p| p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().to_string())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

async fn analyze_diff(
    name: &str,
    kind: ArtifactKind,
    installed_version: &str,
    source_version: &str,
    diff_text: &str,
) -> Result<String> {
    let cfg = config::load_config()?;

    let gateway: Arc<dyn LlmGateway + Send + Sync> = match cfg.llm.gateway {
        LlmGatewayType::OpenAI => Arc::new(OpenAIGateway::default()),
        LlmGatewayType::Ollama => Arc::new(OllamaGateway::new()),
    };

    let broker = LlmBroker::new(&cfg.llm.model, gateway, None);

    let system_prompt = "You are a technical analyst comparing two versions of an AI coding assistant artifact (an agent definition or skill definition written in markdown). \
        Provide a clear, concise summary of the differences. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed\n\
        2. Whether the update is significant or cosmetic\n\
        3. A recommendation: should the user update their installed version?\n\n\
        Keep your analysis brief and actionable — a few paragraphs at most.";

    let user_prompt = format!(
        "Compare these two versions of the {kind} '{name}':\n\
        - Installed version: {installed_version}\n\
        - Source version: {source_version}\n\n\
        {diff_text}"
    );

    let messages = vec![
        LlmMessage::system(system_prompt),
        LlmMessage::user(&user_prompt),
    ];

    let response = broker
        .generate(&messages, None, None, None)
        .await
        .context("LLM analysis failed")?;

    Ok(response)
}
