use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::checksum;
use crate::config;
use crate::config::InstalledWithSources;
use crate::context::{AppContext, LoadedState};
use crate::source_iter;
use crate::source_iter::SourceArtifactInfo;
use crate::source_update;
use crate::types::{ArtifactKind, LockFile, display_version};

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
    source_update::auto_update_all_with(ctx)?;

    let loaded = LoadedState::load(ctx)?;
    let source_artifacts = source_iter::scan_all_with_checksums(&loaded.sources.sources, ctx.fs)?;

    let mut rows = Vec::new();

    for (local, lock) in [(false, &loaded.global_lock), (true, &loaded.local_lock)] {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            collect_outdated_for_scope_with(kind, local, lock, &source_artifacts, &mut rows, ctx)?;
        }
    }

    // Deduplicate by (name, source) in case both lock and disk scan find the same artifact
    let mut seen = BTreeSet::new();
    rows.retain(|r| seen.insert((r.name.clone(), r.source.clone())));

    Ok(rows)
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
fn outdated_status_label(installed_v: Option<&str>, available_v: Option<&str>) -> &'static str {
    match (installed_v, available_v) {
        (None, Some(_)) => "untracked",
        (Some(i), Some(a)) if i != a => "update",
        _ => "changed",
    }
}

/// Determine whether an installed artifact has been locally modified since
/// installation. Returns `false` if there is no lock entry or the file is not
/// present on disk.
fn check_locally_modified(
    lock_entry: Option<&crate::types::LockEntry>,
    kind: ArtifactKind,
    name: &str,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<bool> {
    let Some(entry) = lock_entry else {
        return Ok(false);
    };
    let install_path = kind.installed_path(name, &ctx.paths.install_dir(kind, local));
    if !ctx.fs.exists(&install_path) {
        return Ok(false);
    }
    checksum::is_locally_modified(&install_path, kind, entry, ctx.fs)
}

fn collect_outdated_for_scope_with(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_artifacts: &BTreeMap<String, Vec<SourceArtifactInfo>>,
    rows: &mut Vec<OutdatedRow>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let pairs =
        config::match_installed_to_sources(kind, local, lock, source_artifacts, ctx.fs, ctx.paths)?;
    let names: Vec<&str> = pairs.iter().map(|(ia, _)| ia.name.as_str()).collect();
    let modifications = compute_modification_status(kind, local, &names, lock, ctx)?;
    rows.extend(compare_versions(kind, pairs, &modifications));
    Ok(())
}

/// Pre-compute whether each artifact has been locally modified since installation.
fn compute_modification_status(
    kind: ArtifactKind,
    local: bool,
    names: &[&str],
    lock: &LockFile,
    ctx: &AppContext<'_>,
) -> Result<HashMap<String, bool>> {
    names
        .iter()
        .map(|&name| {
            let lock_entry = lock.packages.get(name);
            let modified = check_locally_modified(lock_entry, kind, name, local, ctx)?;
            Ok((name.to_string(), modified))
        })
        .collect()
}

/// Pure comparison — no filesystem access. Accepts pre-computed pairs and
/// pre-loaded modification status.
fn compare_versions(
    kind: ArtifactKind,
    pairs: Vec<InstalledWithSources<'_, SourceArtifactInfo>>,
    modifications: &HashMap<String, bool>,
) -> Vec<OutdatedRow> {
    let mut rows = Vec::new();

    for (ia, source_infos) in pairs {
        let lock_entry = ia.lock_entry;

        // No source artifact — nothing to compare against
        let Some(source_infos) = source_infos else {
            continue;
        };

        let installed_v: Option<String> = ia.installed_version.clone();
        let locally_modified = modifications.get(&ia.name).copied().unwrap_or(false);

        for source_info in source_infos {
            let available_v: Option<String> = source_info.version.clone();

            if !is_outdated(lock_entry, &source_info.checksum, source_info.version.as_deref()) {
                continue;
            }

            let mut status =
                outdated_status_label(installed_v.as_deref(), available_v.as_deref()).to_string();
            if locally_modified {
                status = format!("{status} (modified)");
            }

            rows.push(OutdatedRow {
                name: ia.name.clone(),
                kind,
                installed_version: display_version(installed_v.as_deref()).to_string(),
                available_version: display_version(available_v.as_deref()).to_string(),
                source: source_info.source_name.clone(),
                status,
            });
        }
    }

    rows
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
        save_lock_with_entry, setup_empty_sources, setup_source_with_versioned_agent,
        setup_sources, test_paths, versioned_agent_content,
    };
    use crate::types::{ArtifactKind, InstalledArtifact, LockFile};
    use chrono::Utc;
    use std::collections::{BTreeMap, HashMap};

    // --- compare_versions (pure, no gateway fakes needed) ---

    #[test]
    fn compare_versions_emits_outdated_row_when_checksum_differs() {
        let mut packages = BTreeMap::new();
        packages.insert(
            "my-agent".to_string(),
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "agents/my-agent.md",
                "sha256:old",
            ),
        );
        let lock = LockFile {
            version: 1,
            packages,
        };

        let ia = InstalledArtifact {
            name: "my-agent".to_string(),
            lock_entry: lock.packages.get("my-agent"),
            installed_version: Some("1.0.0".to_string()),
        };
        let source_infos = vec![crate::source_iter::SourceArtifactInfo {
            source_name: "guidelines".to_string(),
            version: Some("2.0.0".to_string()),
            checksum: "sha256:new".to_string(),
            deprecated: false,
        }];
        let pairs = vec![(ia, Some(&source_infos))];
        let modifications = HashMap::from([("my-agent".to_string(), false)]);

        let rows = compare_versions(ArtifactKind::Agent, pairs, &modifications);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "my-agent");
        assert_eq!(rows[0].available_version, "2.0.0");
        assert_eq!(rows[0].status, "update");
    }

    #[test]
    fn compare_versions_appends_modified_suffix_when_locally_modified() {
        let mut packages = BTreeMap::new();
        packages.insert(
            "my-agent".to_string(),
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "agents/my-agent.md",
                "sha256:old",
            ),
        );
        let lock = LockFile {
            version: 1,
            packages,
        };

        let ia = InstalledArtifact {
            name: "my-agent".to_string(),
            lock_entry: lock.packages.get("my-agent"),
            installed_version: Some("1.0.0".to_string()),
        };
        let source_infos = vec![crate::source_iter::SourceArtifactInfo {
            source_name: "guidelines".to_string(),
            version: Some("2.0.0".to_string()),
            checksum: "sha256:new".to_string(),
            deprecated: false,
        }];
        let pairs = vec![(ia, Some(&source_infos))];
        let modifications = HashMap::from([("my-agent".to_string(), true)]);

        let rows = compare_versions(ArtifactKind::Agent, pairs, &modifications);
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].status.contains("modified"),
            "expected 'modified' in status: {}",
            rows[0].status
        );
    }

    #[test]
    fn compare_versions_skips_pairs_without_source_infos() {
        let lock = LockFile::default();
        let ia = InstalledArtifact {
            name: "orphan".to_string(),
            lock_entry: None,
            installed_version: None,
        };
        let pairs: Vec<_> = vec![(ia, None)];
        let modifications = HashMap::new();

        let rows = compare_versions(ArtifactKind::Agent, pairs, &modifications);
        assert!(rows.is_empty(), "orphan with no source should produce no rows");
        let _ = lock;
    }

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
        assert_eq!(outdated_status_label(None, Some("1.0.0")), "untracked");
    }

    #[test]
    fn status_label_update_available() {
        assert_eq!(outdated_status_label(Some("1.0.0"), Some("2.0.0")), "update");
    }

    #[test]
    fn status_label_changed_same_version() {
        assert_eq!(outdated_status_label(Some("1.0.0"), Some("1.0.0")), "changed");
    }

    #[test]
    fn status_label_changed_no_versions() {
        assert_eq!(outdated_status_label(None, None), "changed");
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
        setup_empty_sources(&fs, &paths);

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
