use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::source_iter::SourceArtifactInfo;
use crate::table::Table;
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
// Public API
// ---------------------------------------------------------------------------

pub fn outdated_with(ctx: &AppContext<'_>) -> Result<Vec<OutdatedRow>> {
    source::auto_update_all_with(ctx)?;

    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let source_artifacts = source_iter::scan_all_with_checksums(&sources.sources, ctx.fs)?;

    let mut rows = Vec::new();

    for local in [false, true] {
        let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            collect_outdated_for_scope_with(kind, local, &lock, &source_artifacts, &mut rows, ctx)?;
        }
    }

    // Deduplicate by (name, source) in case both lock and disk scan find the same artifact
    let mut seen = BTreeSet::new();
    rows.retain(|r| seen.insert((r.name.clone(), r.source.clone())));

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

    Table {
        headers: vec!["Name", "Type", "Installed", "Available", "Source", "Status"],
        padded_cols: 6,
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.kind.to_string(),
                    r.installed_version.clone(),
                    r.available_version.clone(),
                    r.source.clone(),
                    r.status.clone(),
                ]
            })
            .collect(),
    }
    .print();
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
    source_artifacts: &BTreeMap<String, Vec<SourceArtifactInfo>>,
    rows: &mut Vec<OutdatedRow>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let installed = config::installed_names_with(kind, local, ctx.fs, ctx.paths)?;

    for name in &installed {
        let lock_entry = lock.packages.get(name);
        let source_infos = source_artifacts.get(name);

        // No source artifact — nothing to compare against
        let Some(source_infos) = source_infos else {
            continue;
        };

        let installed_v = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-").to_string();

        // Check for local modifications once (shared across all source rows)
        let locally_modified = if let Some(entry) = lock_entry {
            let install_path = kind.installed_path(name, &ctx.paths.install_dir(kind, local));
            ctx.fs.exists(&install_path)
                && checksum::is_locally_modified(&install_path, kind, entry, ctx.fs)?
        } else {
            false
        };

        for source_info in source_infos {
            let available_v = source_info.version.as_deref().unwrap_or("-").to_string();

            if !is_outdated(lock_entry, &source_info.checksum, source_info.version.as_deref()) {
                continue;
            }

            let mut status = outdated_status_label(&installed_v, &available_v).to_string();
            if locally_modified {
                status = format!("{status} (modified)");
            }

            rows.push(OutdatedRow {
                name: name.clone(),
                kind,
                installed_version: installed_v.clone(),
                available_version: available_v,
                source: source_info.source_name.clone(),
                status,
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::lockfile;
    use crate::test_support::{
        agent_content, install_agent_on_disk, make_ctx, make_lock_entry_with_checksum,
        save_lock_with_entry, setup_source_with_versioned_agent, setup_sources, test_paths,
        versioned_agent_content,
    };
    use crate::types::{ArtifactKind, LockFile, SourcesFile};
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn make_lock_entry(source_checksum: &str, version: Option<&str>) -> crate::types::LockEntry {
        make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            version,
            "guidelines",
            "agents/my-agent.md",
            source_checksum,
        )
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

    // --- outdated_with ---

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
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "my-agent.md",
                "sha256:old",
            ),
            false,
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = outdated_with(&ctx).unwrap();

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
        let rows = outdated_with(&ctx).unwrap();

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
        lockfile::save_with(
            &LockFile {
                version: 1,
                packages: BTreeMap::new(),
            },
            false,
            &fs,
            &paths,
        )
        .unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = outdated_with(&ctx).unwrap();

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

        // Lock entry says installed_checksum doesn't match disk content
        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "guidelines",
            "my-agent.md",
            "sha256:old",
        );
        // Installed checksum doesn't match disk content
        entry.installed_checksum = "sha256:different".to_string();
        save_lock_with_entry(&fs, &paths, "my-agent", entry, false);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = outdated_with(&ctx).unwrap();

        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].status.contains("modified"),
            "status should contain 'modified': {}",
            rows[0].status
        );
    }

    #[test]
    fn outdated_shows_rows_from_both_sources_when_artifact_in_two() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Two sources with the same agent at different versions
        setup_sources(
            &fs,
            &paths,
            &[
                ("guidelines", "/sources/guidelines"),
                ("marketplace", "/sources/marketplace"),
            ],
        );
        fs.add_file(
            "/sources/guidelines/agents/my-agent.md",
            versioned_agent_content("my-agent", "A test agent", "2.0.0"),
        );
        fs.add_file(
            "/sources/marketplace/agents/my-agent.md",
            versioned_agent_content("my-agent", "A test agent", "3.0.0"),
        );

        // Install on disk with version 1.0.0
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            false,
        );
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "my-agent.md",
                "sha256:old",
            ),
            false,
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let rows = outdated_with(&ctx).unwrap();

        assert_eq!(rows.len(), 2, "should show outdated row for each source");

        let source_names: Vec<&str> = rows.iter().map(|r| r.source.as_str()).collect();
        assert!(source_names.contains(&"guidelines"));
        assert!(source_names.contains(&"marketplace"));

        let guidelines_row = rows.iter().find(|r| r.source == "guidelines").unwrap();
        let marketplace_row = rows.iter().find(|r| r.source == "marketplace").unwrap();
        assert_eq!(guidelines_row.available_version, "2.0.0");
        assert_eq!(marketplace_row.available_version, "3.0.0");
    }
}
