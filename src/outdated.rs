use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::lockfile;
use crate::scan;
use crate::source;
use crate::types::{ArtifactKind, LockFile};

struct OutdatedRow {
    name: String,
    kind: ArtifactKind,
    installed_version: String,
    available_version: String,
    source: String,
    status: String,
}

pub fn outdated() -> Result<()> {
    source::auto_update_all()?;

    let source_artifacts = scan_all_sources()?;
    let global_lock = lockfile::load(false)?;
    let local_lock = lockfile::load(true)?;

    let mut rows = Vec::new();

    // Check all installed artifacts (both tracked and untracked)
    collect_outdated_for_scope(ArtifactKind::Agent, false, &global_lock, &source_artifacts, &mut rows)?;
    collect_outdated_for_scope(ArtifactKind::Skill, false, &global_lock, &source_artifacts, &mut rows)?;
    collect_outdated_for_scope(ArtifactKind::Agent, true, &local_lock, &source_artifacts, &mut rows)?;
    collect_outdated_for_scope(ArtifactKind::Skill, true, &local_lock, &source_artifacts, &mut rows)?;

    // Deduplicate by name (in case both lock and disk scan find the same artifact)
    let mut seen = BTreeSet::new();
    rows.retain(|r| seen.insert(r.name.clone()));

    if rows.is_empty() {
        println!("Everything is up to date.");
        return Ok(());
    }

    let w_name = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let w_kind = 5;
    let w_iv = rows.iter().map(|r| r.installed_version.len()).max().unwrap_or(9).max(9);
    let w_av = rows.iter().map(|r| r.available_version.len()).max().unwrap_or(9).max(9);
    let w_src = rows.iter().map(|r| r.source.len()).max().unwrap_or(6).max(6);
    let w_st = rows.iter().map(|r| r.status.len()).max().unwrap_or(6).max(6);

    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_iv$}  {:<w_av$}  {:<w_src$}  {:<w_st$}",
        "Name", "Type", "Installed", "Available", "Source", "Status",
    );
    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_iv$}  {:<w_av$}  {:<w_src$}  {:<w_st$}",
        "-".repeat(w_name),
        "-".repeat(w_kind),
        "-".repeat(w_iv),
        "-".repeat(w_av),
        "-".repeat(w_src),
        "-".repeat(w_st),
    );

    for row in &rows {
        println!(
            "  {:<w_name$}  {:<w_kind$}  {:<w_iv$}  {:<w_av$}  {:<w_src$}  {:<w_st$}",
            row.name,
            row.kind,
            row.installed_version,
            row.available_version,
            row.source,
            row.status,
        );
    }

    Ok(())
}

fn collect_outdated_for_scope(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_artifacts: &BTreeMap<String, SourceArtifactInfo>,
    rows: &mut Vec<OutdatedRow>,
) -> Result<()> {
    let installed = installed_names(kind, local)?;

    for name in &installed {
        let lock_entry = lock.packages.get(name);
        let source_info = source_artifacts.get(name);

        // No source artifact — nothing to compare against
        let Some(source_info) = source_info else {
            continue;
        };

        let installed_v = lock_entry
            .and_then(|e| e.version.as_deref())
            .unwrap_or("-")
            .to_string();

        let available_v = source_info
            .version
            .as_deref()
            .unwrap_or("-")
            .to_string();

        // Determine if outdated
        let is_outdated = match lock_entry {
            Some(entry) => {
                // Has lock entry — check checksum
                if entry.source_checksum != source_info.checksum {
                    true
                } else if entry.version.is_none() && source_info.version.is_some() {
                    // Installed without version, source now has one
                    true
                } else {
                    false
                }
            }
            None => {
                // No lock entry at all — untracked, source has a versioned copy
                true
            }
        };

        if !is_outdated {
            continue;
        }

        let status = if installed_v == "-" && available_v != "-" {
            "untracked".to_string()
        } else if installed_v != "-" && available_v != "-" && installed_v != available_v {
            "update".to_string()
        } else {
            "changed".to_string()
        };

        rows.push(OutdatedRow {
            name: name.clone(),
            kind,
            installed_version: installed_v,
            available_version: available_v,
            source: source_info.source_name.clone(),
            status,
        });
    }

    Ok(())
}

struct SourceArtifactInfo {
    source_name: String,
    version: Option<String>,
    checksum: String,
}

fn scan_all_sources() -> Result<BTreeMap<String, SourceArtifactInfo>> {
    let sources = config::load_sources()?;
    let mut result = BTreeMap::new();

    for (source_name, entry) in &sources.sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in &artifacts {
                let cs = match artifact.artifact_kind() {
                    ArtifactKind::Agent => checksum::checksum_file(artifact.path())?,
                    ArtifactKind::Skill => checksum::checksum_dir(artifact.path())?,
                };
                result.insert(
                    artifact.name().to_string(),
                    SourceArtifactInfo {
                        source_name: source_name.clone(),
                        version: artifact.version().map(|v| v.to_string()),
                        checksum: cs,
                    },
                );
            }
        }
    }

    Ok(result)
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
