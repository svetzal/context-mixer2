use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::{ArtifactKind, LockEntry, LockSource};

pub fn install(name: &str, kind: ArtifactKind, local: bool, force: bool) -> Result<()> {
    let (source_name, artifact_name) = parse_name(name);

    // Auto-update stale sources before searching
    source::auto_update_all()?;

    let sources = config::load_sources()?;

    if sources.sources.is_empty() {
        bail!("No sources registered. Add one with: cmx source add <name> <path-or-url>");
    }

    // Search sources for the artifact
    let mut found: Vec<(String, ArtifactKind, PathBuf, PathBuf, Option<String>)> = Vec::new();

    let search_sources: std::collections::BTreeMap<_, _> = if let Some(src) = source_name {
        let entry =
            sources.sources.get(src).with_context(|| format!("Source '{src}' not found."))?;
        std::iter::once((src.to_string(), entry.clone())).collect()
    } else {
        sources.sources.clone()
    };

    for sa in source_iter::each_source_artifact(&search_sources) {
        if sa.artifact.name == artifact_name && sa.artifact.kind == kind {
            found.push((
                sa.source_name,
                sa.artifact.kind,
                sa.artifact.path,
                sa.source_root,
                sa.artifact.version,
            ));
        }
    }

    if found.is_empty() {
        bail!("No {kind} named '{artifact_name}' found in registered sources.",);
    }

    if found.len() > 1 {
        let source_names: Vec<_> = found.iter().map(|(s, _, _, _, _)| s.as_str()).collect();
        bail!(
            "'{artifact_name}' found in multiple sources: {}. Use <source>:{artifact_name} to disambiguate.",
            source_names.join(", ")
        );
    }

    let (source_name, _found_kind, artifact_path, source_root, artifact_version) = found.remove(0);
    let dest_dir = config::install_dir(kind, local)?;

    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;

    // Compute source checksum before copying
    let source_checksum = checksum::checksum_artifact(&artifact_path, kind)?;

    // Compute relative path within the source repo
    let relative_path = artifact_path
        .strip_prefix(&source_root)
        .unwrap_or(&artifact_path)
        .to_string_lossy()
        .to_string();

    // Check for local modifications before overwriting
    if !force {
        let dest_check = kind.installed_path(artifact_name, &dest_dir);
        if dest_check.exists() {
            let lock = lockfile::load(local)?;
            if let Some(entry) = lock.packages.get(artifact_name) {
                let current_cs = checksum::checksum_artifact(&dest_check, kind)?;
                if current_cs != entry.installed_checksum {
                    bail!(
                        "'{artifact_name}' has local modifications. Use --force to overwrite, \
                         or 'cmx {kind} diff {artifact_name}' to review changes first."
                    );
                }
            }
        }
    }

    let dest_path = match kind {
        ArtifactKind::Agent => {
            let filename = artifact_path.file_name().context("Invalid agent path")?;
            let dest = dest_dir.join(filename);
            fs::copy(&artifact_path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", artifact_path.display(), dest.display())
            })?;
            dest
        }
        ArtifactKind::Skill => {
            let dir_name = artifact_path.file_name().context("Invalid skill path")?;
            let dest = dest_dir.join(dir_name);
            copy_dir_recursive(&artifact_path, &dest)?;
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
    let installed_checksum = checksum::checksum_artifact(&dest_path, kind)?;

    // Record in lock file
    let version_info = artifact_version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
    let mut lock = lockfile::load(local)?;
    lock.packages.insert(
        artifact_name.to_string(),
        LockEntry {
            artifact_type: kind,
            version: artifact_version,
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

    for sa in source_iter::each_source_artifact(&sources.sources) {
        if sa.artifact.kind != kind {
            continue;
        }
        // Skip if already tracked with matching version AND checksum
        if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
            let source_cs = checksum::checksum_artifact(&sa.artifact.path, kind)?;
            if lock_entry.version.as_deref() == sa.artifact.version.as_deref()
                && lock_entry.source_checksum == source_cs
            {
                continue;
            }
        }
        let pinned = format!("{}:{}", sa.source_name, sa.artifact.name);
        install(&pinned, kind, local, force)?;
        installed_count += 1;
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
            if entry.artifact_type != kind {
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
    let sources = config::load_sources()?;
    let mut checksums = std::collections::BTreeMap::new();

    for sa in source_iter::each_source_artifact(&sources.sources) {
        if sa.artifact.kind == kind {
            let cs = checksum::checksum_artifact(&sa.artifact.path, kind)?;
            checksums.insert(sa.artifact.name, cs);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_with_source_prefix() {
        let (source, artifact) = parse_name("guidelines:rust-craftsperson");
        assert_eq!(source, Some("guidelines"));
        assert_eq!(artifact, "rust-craftsperson");
    }

    #[test]
    fn parse_name_without_source_prefix() {
        let (source, artifact) = parse_name("rust-craftsperson");
        assert_eq!(source, None);
        assert_eq!(artifact, "rust-craftsperson");
    }

    #[test]
    fn parse_name_splits_on_first_colon_only() {
        // "a:b:c" — split_once splits only at the first colon
        let (source, artifact) = parse_name("a:b:c");
        assert_eq!(source, Some("a"));
        assert_eq!(artifact, "b:c");
    }

    #[test]
    fn parse_name_empty_source() {
        let (source, artifact) = parse_name(":artifact");
        // split_once(":") returns Some(("", "artifact"))
        assert_eq!(source, Some(""));
        assert_eq!(artifact, "artifact");
    }

    #[test]
    fn parse_name_empty_artifact() {
        let (source, artifact) = parse_name("source:");
        assert_eq!(source, Some("source"));
        assert_eq!(artifact, "");
    }
}
