use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::{ArtifactKind, LockFile};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub struct OutdatedRow {
    pub name: String,
    pub kind: ArtifactKind,
    pub installed_version: String,
    pub available_version: String,
    pub source: String,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn outdated_with(ctx: &AppContext<'_>) -> Result<Vec<OutdatedRow>> {
    gather_outdated_with(ctx)
}

// ---------------------------------------------------------------------------
// Gather (pure logic, no println!)
// ---------------------------------------------------------------------------

pub(crate) fn gather_outdated_with(ctx: &AppContext<'_>) -> Result<Vec<OutdatedRow>> {
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

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Print (no business logic)
// ---------------------------------------------------------------------------

pub fn print_outdated(rows: &[OutdatedRow]) {
    if rows.is_empty() {
        println!("Everything is up to date.");
        return;
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

    for row in rows {
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
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

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
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::lockfile;
    use crate::test_support::{agent_content, install_agent_on_disk, make_ctx, test_paths};
    use crate::types::{
        ArtifactKind, LockEntry, LockFile, LockSource, SourceEntry, SourceType, SourcesFile,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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

    // --- gather_outdated_with ---

    fn setup_source_with_versioned_agent(
        fs: &FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        source_name: &str,
        source_path: &str,
        agent_name: &str,
        version: &str,
    ) {
        let sources = SourcesFile {
            version: 1,
            sources: {
                let mut m = BTreeMap::new();
                m.insert(
                    source_name.to_string(),
                    SourceEntry {
                        source_type: SourceType::Local,
                        path: Some(PathBuf::from(source_path)),
                        url: None,
                        local_clone: None,
                        branch: None,
                        last_updated: Some(Utc::now().to_rfc3339()),
                    },
                );
                m
            },
        };
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        // Source has version embedded in frontmatter
        fs.add_file(
            format!("{source_path}/{agent_name}.md"),
            format!(
                "---\nname: {agent_name}\ndescription: A test agent\nversion: {version}\n---\n"
            ),
        );
    }

    #[test]
    fn gather_outdated_outdated_artifact_appears_in_rows() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has version 2.0.0
        setup_source_with_versioned_agent(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "2.0.0",
        );

        // Install on disk with version 1.0.0 lock entry
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            false,
        );
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "guidelines".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:old".to_string(),
                installed_checksum: "sha256:old".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = gather_outdated_with(&ctx).unwrap();

        assert_eq!(rows.len(), 1, "expected one outdated artifact");
        assert_eq!(rows[0].name, "my-agent");
        assert_eq!(rows[0].installed_version, "1.0.0");
        assert_eq!(rows[0].available_version, "2.0.0");
        assert_eq!(rows[0].source, "guidelines");
    }

    #[test]
    fn gather_outdated_up_to_date_returns_empty() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // No installed artifacts, no sources
        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = gather_outdated_with(&ctx).unwrap();

        assert!(rows.is_empty(), "expected no rows when everything is up to date");
    }

    #[test]
    fn gather_outdated_untracked_artifact_appears_as_untracked() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has version 1.0.0
        setup_source_with_versioned_agent(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "1.0.0",
        );

        // Installed on disk but NOT in lock file
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            false,
        );
        // Empty lock file — no lock entry
        let lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = gather_outdated_with(&ctx).unwrap();

        assert_eq!(rows.len(), 1, "untracked artifact should appear");
        assert_eq!(rows[0].name, "my-agent");
        assert_eq!(rows[0].installed_version, "-");
        assert_eq!(rows[0].status, "untracked");
    }

    #[test]
    fn gather_outdated_locally_modified_appends_modified_status() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has version 2.0.0
        setup_source_with_versioned_agent(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "2.0.0",
        );

        // Install on disk
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            false,
        );

        // Lock entry says installed_checksum matches "sha256:lock_cs" but disk has different content
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "guidelines".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:old".to_string(),
                // Installed checksum doesn't match disk content
                installed_checksum: "sha256:different".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = gather_outdated_with(&ctx).unwrap();

        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].status.contains("modified"),
            "status should contain 'modified': {}",
            rows[0].status
        );
    }
}
