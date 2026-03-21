use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::lockfile;
use crate::scan;
use crate::source;
use crate::types::{Artifact, ArtifactKind, ArtifactKindSerde, LockEntry, LockSource};

pub fn install(name: &str, kind: ArtifactKind, local: bool, force: bool) -> Result<()> {
    let (source_name, artifact_name) = parse_name(name);

    // Auto-update stale sources before searching
    source::auto_update_all()?;

    let sources = config::load_sources()?;

    if sources.sources.is_empty() {
        bail!("No sources registered. Add one with: cmx source add <name> <path-or-url>");
    }

    // Search sources for the artifact
    let mut found: Vec<(String, Artifact, PathBuf)> = Vec::new();

    let search_sources: Vec<_> = if let Some(src) = source_name {
        let entry =
            sources.sources.get(src).with_context(|| format!("Source '{src}' not found."))?;
        vec![(src.to_string(), entry.clone())]
    } else {
        sources.sources.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };

    for (sname, entry) in &search_sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        let artifacts = scan::scan_source(&local_path)?;
        for artifact in artifacts {
            if artifact.name() == artifact_name && artifact.artifact_kind() == kind {
                found.push((sname.clone(), artifact, local_path.clone()));
            }
        }
    }

    if found.is_empty() {
        bail!("No {kind} named '{artifact_name}' found in registered sources.",);
    }

    if found.len() > 1 {
        let source_names: Vec<_> = found.iter().map(|(s, _, _)| s.as_str()).collect();
        bail!(
            "'{artifact_name}' found in multiple sources: {}. Use <source>:{artifact_name} to disambiguate.",
            source_names.join(", ")
        );
    }

    let (source_name, artifact, source_root) = found.remove(0);
    let dest_dir = config::install_dir(kind, local)?;

    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;

    // Compute source checksum before copying
    let source_checksum = match &artifact {
        Artifact::Agent { path, .. } => checksum::checksum_file(path)?,
        Artifact::Skill { path, .. } => checksum::checksum_dir(path)?,
    };

    // Compute relative path within the source repo
    let relative_path = artifact
        .path()
        .strip_prefix(&source_root)
        .unwrap_or(artifact.path())
        .to_string_lossy()
        .to_string();

    // Check for local modifications before overwriting
    if !force {
        let dest_check = match kind {
            ArtifactKind::Agent => dest_dir.join(format!("{artifact_name}.md")),
            ArtifactKind::Skill => dest_dir.join(artifact_name),
        };
        if dest_check.exists() {
            let lock = lockfile::load(local)?;
            if let Some(entry) = lock.packages.get(artifact_name) {
                let current_cs = match kind {
                    ArtifactKind::Agent => checksum::checksum_file(&dest_check)?,
                    ArtifactKind::Skill => checksum::checksum_dir(&dest_check)?,
                };
                if current_cs != entry.installed_checksum {
                    bail!(
                        "'{artifact_name}' has local modifications. Use --force to overwrite, \
                         or 'cmx {kind} diff {artifact_name}' to review changes first."
                    );
                }
            }
        }
    }

    let dest_path = match &artifact {
        Artifact::Agent { path, .. } => {
            let filename = path.file_name().context("Invalid agent path")?;
            let dest = dest_dir.join(filename);
            fs::copy(path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", path.display(), dest.display())
            })?;
            dest
        }
        Artifact::Skill { path, .. } => {
            let dir_name = path.file_name().context("Invalid skill path")?;
            let dest = dest_dir.join(dir_name);
            copy_dir_recursive(path, &dest)?;
            dest
        }
    };

    // Validate skill installation
    if matches!(kind, ArtifactKind::Skill) {
        let skill_md = dest_path.join("SKILL.md");
        if !skill_md.exists() {
            let _ = fs::remove_dir_all(&dest_path);
            bail!("Skill '{}' is missing SKILL.md. Partial install removed.", artifact_name);
        }
    }

    // Compute installed checksum from what was actually written to disk
    let installed_checksum = match kind {
        ArtifactKind::Agent => checksum::checksum_file(&dest_path)?,
        ArtifactKind::Skill => checksum::checksum_dir(&dest_path)?,
    };

    // Record in lock file
    let mut lock = lockfile::load(local)?;
    lock.packages.insert(
        artifact_name.to_string(),
        LockEntry {
            artifact_type: match kind {
                ArtifactKind::Agent => ArtifactKindSerde::Agent,
                ArtifactKind::Skill => ArtifactKindSerde::Skill,
            },
            version: artifact.version().map(|v| v.to_string()),
            installed_at: Utc::now().to_rfc3339(),
            source: LockSource {
                repo: source_name.clone(),
                path: relative_path,
            },
            source_checksum,
            installed_checksum,
        },
    );
    lockfile::save(&lock, local)?;

    let version_info = artifact.version().map(|v| format!(" v{v}")).unwrap_or_default();
    println!(
        "Installed {artifact_name}{version_info} ({kind}) from '{source_name}' -> {}",
        dest_dir.display()
    );

    Ok(())
}

pub fn update(name: &str, kind: ArtifactKind, force: bool) -> Result<()> {
    // Find which scope it's installed in
    for local in [false, true] {
        let lock = lockfile::load(local)?;
        if let Some(entry) = lock.packages.get(name) {
            let pinned = format!("{}:{}", entry.source.repo, name);
            return install(&pinned, kind, local, force);
        }
    }

    bail!(
        "No installed {kind} named '{name}' found. Install it first with 'cmx {kind} install {name}'."
    );
}

pub fn install_all(kind: ArtifactKind, local: bool, force: bool) -> Result<()> {
    source::auto_update_all()?;

    let sources = config::load_sources()?;
    let lock = lockfile::load(local)?;
    let mut installed_count = 0;

    for (source_name, entry) in &sources.sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in &artifacts {
                if artifact.artifact_kind() != kind {
                    continue;
                }
                // Skip if already tracked with matching version AND checksum
                if let Some(lock_entry) = lock.packages.get(artifact.name()) {
                    let source_cs = match kind {
                        ArtifactKind::Agent => checksum::checksum_file(artifact.path())?,
                        ArtifactKind::Skill => checksum::checksum_dir(artifact.path())?,
                    };
                    if lock_entry.version.as_deref() == artifact.version()
                        && lock_entry.source_checksum == source_cs
                    {
                        continue;
                    }
                }
                let pinned = format!("{source_name}:{}", artifact.name());
                install(&pinned, kind, local, force)?;
                installed_count += 1;
            }
        }
    }

    if installed_count == 0 {
        println!("All available {kind}s are already installed and up to date.");
    }

    Ok(())
}

pub fn update_all(kind: ArtifactKind, force: bool) -> Result<()> {
    source::auto_update_all()?;

    // Scan sources for current checksums
    let source_checksums = scan_source_checksums(kind)?;
    let mut updated_count = 0;

    for local in [false, true] {
        let lock = lockfile::load(local)?;
        for (name, entry) in &lock.packages {
            let entry_kind = match entry.artifact_type {
                ArtifactKindSerde::Agent => ArtifactKind::Agent,
                ArtifactKindSerde::Skill => ArtifactKind::Skill,
            };
            if entry_kind != kind {
                continue;
            }

            if let Some(source_cs) = source_checksums.get(name)
                && entry.source_checksum != *source_cs
            {
                let pinned = format!("{}:{name}", entry.source.repo);
                install(&pinned, kind, local, force)?;
                updated_count += 1;
            }
        }
    }

    if updated_count == 0 {
        println!("All tracked {kind}s are up to date.");
    }

    Ok(())
}

fn scan_source_checksums(kind: ArtifactKind) -> Result<std::collections::BTreeMap<String, String>> {
    use crate::checksum;
    let sources = config::load_sources()?;
    let mut checksums = std::collections::BTreeMap::new();

    for entry in sources.sources.values() {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in &artifacts {
                if artifact.artifact_kind() == kind {
                    let cs = match kind {
                        ArtifactKind::Agent => checksum::checksum_file(artifact.path())?,
                        ArtifactKind::Skill => checksum::checksum_dir(artifact.path())?,
                    };
                    checksums.insert(artifact.name().to_string(), cs);
                }
            }
        }
    }

    Ok(checksums)
}

fn parse_name(name: &str) -> (Option<&str>, &str) {
    if let Some((source, artifact)) = name.split_once(':') {
        (Some(source), artifact)
    } else {
        (None, name)
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("Failed to create {}", dest.display()))?;

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
