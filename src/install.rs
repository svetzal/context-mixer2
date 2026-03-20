use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::scan;
use crate::types::{Artifact, ArtifactKind};

pub fn install(name: &str, kind: ArtifactKind, local: bool) -> Result<()> {
    let (source_name, artifact_name) = parse_name(name);

    let sources = config::load_sources()?;

    if sources.sources.is_empty() {
        bail!("No sources registered. Add one with: cmx source add <name> <path-or-url>");
    }

    // Search sources for the artifact
    let mut found: Vec<(String, Artifact)> = Vec::new();

    let search_sources: Vec<_> = if let Some(src) = source_name {
        let entry = sources
            .sources
            .get(src)
            .with_context(|| format!("Source '{src}' not found."))?;
        vec![(src.to_string(), entry.clone())]
    } else {
        sources
            .sources
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    for (sname, entry) in &search_sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        let artifacts = scan::scan_source(&local_path)?;
        for artifact in artifacts {
            if artifact.name() == artifact_name && artifact.artifact_kind() == kind {
                found.push((sname.clone(), artifact));
            }
        }
    }

    if found.is_empty() {
        bail!(
            "No {kind} named '{artifact_name}' found in registered sources.",
        );
    }

    if found.len() > 1 {
        let source_names: Vec<_> = found.iter().map(|(s, _)| s.as_str()).collect();
        bail!(
            "'{artifact_name}' found in multiple sources: {}. Use <source>:{artifact_name} to disambiguate.",
            source_names.join(", ")
        );
    }

    let (source_name, artifact) = found.remove(0);
    let dest_dir = install_dir(kind, local)?;

    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;

    match &artifact {
        Artifact::Agent { path, .. } => {
            let filename = path.file_name().context("Invalid agent path")?;
            let dest = dest_dir.join(filename);
            fs::copy(path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", path.display(), dest.display())
            })?;
        }
        Artifact::Skill { path, .. } => {
            let dir_name = path.file_name().context("Invalid skill path")?;
            let dest = dest_dir.join(dir_name);
            copy_dir_recursive(path, &dest)?;
        }
    }

    let scope = if local { "local" } else { "global" };
    println!(
        "Installed {artifact_name} ({kind}) from '{source_name}' -> {}",
        dest_dir.display()
    );
    println!("Scope: {scope}");

    Ok(())
}

fn install_dir(kind: ArtifactKind, local: bool) -> Result<PathBuf> {
    if local {
        let subdir = match kind {
            ArtifactKind::Agent => "agents",
            ArtifactKind::Skill => "skills",
        };
        Ok(PathBuf::from(".claude").join(subdir))
    } else {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let subdir = match kind {
            ArtifactKind::Agent => "agents",
            ArtifactKind::Skill => "skills",
        };
        Ok(home.join(".claude").join(subdir))
    }
}

fn parse_name(name: &str) -> (Option<&str>, &str) {
    if let Some((source, artifact)) = name.split_once(':') {
        (Some(source), artifact)
    } else {
        (None, name)
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create {}", dest.display()))?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}
