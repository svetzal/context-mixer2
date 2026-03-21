use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::source;
use crate::source_iter;
use crate::types::{ArtifactKind, LockFile};

struct OutdatedRow {
    name: String,
    kind: ArtifactKind,
    installed_version: String,
    available_version: String,
    source: String,
    status: String,
}

pub fn outdated_with(ctx: &AppContext<'_>) -> Result<()> {
    source::auto_update_all_with(ctx)?;

    let source_artifacts = scan_all_sources_with(ctx)?;
    let global_lock = lockfile::load_with(false, ctx.fs, ctx.paths)?;
    let local_lock = lockfile::load_with(true, ctx.fs, ctx.paths)?;

    let mut rows = Vec::new();

    collect_outdated_for_scope_with(
        ArtifactKind::Agent,
        false,
        &global_lock,
        &source_artifacts,
        &mut rows,
        ctx,
    )?;
    collect_outdated_for_scope_with(
        ArtifactKind::Skill,
        false,
        &global_lock,
        &source_artifacts,
        &mut rows,
        ctx,
    )?;
    collect_outdated_for_scope_with(
        ArtifactKind::Agent,
        true,
        &local_lock,
        &source_artifacts,
        &mut rows,
        ctx,
    )?;
    collect_outdated_for_scope_with(
        ArtifactKind::Skill,
        true,
        &local_lock,
        &source_artifacts,
        &mut rows,
        ctx,
    )?;

    // Deduplicate by name (in case both lock and disk scan find the same artifact)
    let mut seen = BTreeSet::new();
    rows.retain(|r| seen.insert(r.name.clone()));

    if rows.is_empty() {
        println!("Everything is up to date.");
        return Ok(());
    }

    let w_name = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let w_kind = 5;
    let w_installed = rows.iter().map(|r| r.installed_version.len()).max().unwrap_or(9).max(9);
    let w_available = rows.iter().map(|r| r.available_version.len()).max().unwrap_or(9).max(9);
    let w_src = rows.iter().map(|r| r.source.len()).max().unwrap_or(6).max(6);
    let w_st = rows.iter().map(|r| r.status.len()).max().unwrap_or(6).max(6);

    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_installed$}  {:<w_available$}  {:<w_src$}  {:<w_st$}",
        "Name", "Type", "Installed", "Available", "Source", "Status",
    );
    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_installed$}  {:<w_available$}  {:<w_src$}  {:<w_st$}",
        "-".repeat(w_name),
        "-".repeat(w_kind),
        "-".repeat(w_installed),
        "-".repeat(w_available),
        "-".repeat(w_src),
        "-".repeat(w_st),
    );

    for row in &rows {
        println!(
            "  {:<w_name$}  {:<w_kind$}  {:<w_installed$}  {:<w_available$}  {:<w_src$}  {:<w_st$}",
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

/// Returns `true` if an installed artifact is considered outdated relative to
/// the source.  Pure function — no I/O.
fn is_outdated(
    lock_entry: Option<&crate::types::LockEntry>,
    source_checksum: &str,
    source_version: Option<&str>,
) -> bool {
    match lock_entry {
        Some(entry) => {
            // Checksum changed
            if entry.source_checksum != source_checksum {
                return true;
            }
            // Installed without a version but source now has one
            if entry.version.is_none() && source_version.is_some() {
                return true;
            }
            false
        }
        // No lock entry — untracked
        None => true,
    }
}

/// Derives the human-readable status label given installed and available version
/// strings.  Pure function — no I/O.
fn outdated_status_label(installed_v: &str, available_v: &str) -> &'static str {
    if installed_v == "-" && available_v != "-" {
        "untracked"
    } else if installed_v != "-" && available_v != "-" && installed_v != available_v {
        "update"
    } else {
        "changed"
    }
}

fn collect_outdated_for_scope_with(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_artifacts: &BTreeMap<String, SourceArtifactInfo>,
    rows: &mut Vec<OutdatedRow>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let installed = config::installed_names_with(kind, local, ctx.fs, ctx.paths)?;

    for name in &installed {
        let lock_entry = lock.packages.get(name);
        let source_info = source_artifacts.get(name);

        // No source artifact — nothing to compare against
        let Some(source_info) = source_info else {
            continue;
        };

        let installed_v = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-").to_string();

        let available_v = source_info.version.as_deref().unwrap_or("-").to_string();

        if !is_outdated(lock_entry, &source_info.checksum, source_info.version.as_deref()) {
            continue;
        }

        let mut status = outdated_status_label(&installed_v, &available_v).to_string();

        // Check for local modifications
        if let Some(entry) = lock_entry {
            let install_path = kind.installed_path(name, &ctx.paths.install_dir(kind, local));
            if ctx.fs.exists(&install_path) {
                let current_cs = checksum::checksum_artifact_with(&install_path, kind, ctx.fs)?;
                if current_cs != entry.installed_checksum {
                    status = format!("{status} (modified)");
                }
            }
        }

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

fn scan_all_sources_with(ctx: &AppContext<'_>) -> Result<BTreeMap<String, SourceArtifactInfo>> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let mut result = BTreeMap::new();

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        let cs = checksum::checksum_artifact_with(&sa.artifact.path, sa.artifact.kind, ctx.fs)?;
        result.insert(
            sa.artifact.name,
            SourceArtifactInfo {
                source_name: sa.source_name,
                version: sa.artifact.version,
                checksum: cs,
            },
        );
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Legacy free-function API
// ---------------------------------------------------------------------------

pub fn outdated() -> Result<()> {
    let paths = ConfigPaths::from_env()?;
    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };
    outdated_with(&ctx)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArtifactKind, LockEntry, LockSource};

    fn make_lock_entry(source_checksum: &str, version: Option<&str>) -> LockEntry {
        LockEntry {
            artifact_type: ArtifactKind::Agent,
            version: version.map(std::string::ToString::to_string),
            installed_at: "2024-01-01T00:00:00Z".to_string(),
            source: LockSource {
                repo: "guidelines".to_string(),
                path: "agents/my-agent.md".to_string(),
            },
            source_checksum: source_checksum.to_string(),
            installed_checksum: source_checksum.to_string(),
        }
    }

    // --- is_outdated ---

    #[test]
    fn is_outdated_matching_checksum_not_outdated() {
        let entry = make_lock_entry("sha256:abc", Some("1.0.0"));
        assert!(!is_outdated(Some(&entry), "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn is_outdated_changed_checksum_is_outdated() {
        let entry = make_lock_entry("sha256:abc", Some("1.0.0"));
        assert!(is_outdated(Some(&entry), "sha256:xyz", Some("1.0.0")));
    }

    #[test]
    fn is_outdated_no_lock_entry_is_outdated() {
        assert!(is_outdated(None, "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn is_outdated_version_appeared_in_source_is_outdated() {
        // Installed without a version; source now carries one
        let entry = make_lock_entry("sha256:abc", None);
        assert!(is_outdated(Some(&entry), "sha256:abc", Some("1.0.0")));
    }

    #[test]
    fn is_outdated_both_unversioned_same_checksum_not_outdated() {
        let entry = make_lock_entry("sha256:abc", None);
        assert!(!is_outdated(Some(&entry), "sha256:abc", None));
    }

    // --- outdated_status_label ---

    #[test]
    fn status_label_untracked() {
        assert_eq!(outdated_status_label("-", "1.0.0"), "untracked");
    }

    #[test]
    fn status_label_update_available() {
        assert_eq!(outdated_status_label("1.0.0", "2.0.0"), "update");
    }

    #[test]
    fn status_label_changed_same_version() {
        assert_eq!(outdated_status_label("1.0.0", "1.0.0"), "changed");
    }

    #[test]
    fn status_label_changed_no_versions() {
        assert_eq!(outdated_status_label("-", "-"), "changed");
    }
}
