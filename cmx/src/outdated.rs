//! `cmx outdated` (compare installed vs source).

use crate::error::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::artifact_status::source_outdated;
use crate::checksum;
use crate::config;
use crate::config::InstalledWithSources;
use crate::context::{AppContext, LoadedState};
use crate::source_iter;
use crate::source_iter::SourceArtifactInfo;
use crate::types::{ArtifactKind, InstallScope, LockFile};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// How an installed artifact relates to what's available from its source.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutdatedStatus {
    /// Installed without a recorded version, but a newer source copy exists.
    Untracked,
    /// The installed version differs from the source's available version.
    Outdated,
    /// The source checksum has moved even though the recorded version matches.
    Changed,
}

impl OutdatedStatus {
    /// Short, stable string label used in table and JSON output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Untracked => "untracked",
            Self::Outdated => "outdated",
            Self::Changed => "changed",
        }
    }
}

/// One installed artifact that is out of date relative to a matching source.
#[derive(Clone, Debug, Serialize)]
pub struct OutdatedRow {
    /// Artifact name.
    pub name: String,
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// Global or local install scope this row was found in.
    pub scope: InstallScope,
    /// Version recorded in the lock file, if any.
    pub installed_version: Option<String>,
    /// Version currently offered by the source, if any.
    pub available_version: Option<String>,
    /// Name of the source that offers the newer copy.
    pub source: String,
    /// Classification of how the installed copy differs from the source.
    pub status: OutdatedStatus,
    /// Whether the installed copy has been hand-edited since installation.
    pub locally_modified: bool,
}

/// The full set of outdated rows produced by [`outdated`].
#[derive(Clone, Debug, Serialize)]
pub struct OutdatedReport(pub Vec<OutdatedRow>);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compare every installed agent/skill against its matching source(s) and
/// report which copies are untracked, outdated, or changed.
pub fn outdated(ctx: &AppContext<'_>) -> Result<OutdatedReport> {
    let loaded = LoadedState::load(ctx)?;
    let source_artifacts = source_iter::scan_all_with_checksums(&loaded.sources.sources, ctx.fs)?;

    let mut rows = Vec::new();

    for (scope, lock) in loaded.scopes() {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            collect_outdated_for_scope_with(kind, scope, lock, &source_artifacts, &mut rows, ctx)?;
        }
    }

    let mut seen = BTreeSet::new();
    rows.retain(|r| seen.insert((r.name.clone(), r.source.clone())));

    Ok(OutdatedReport(rows))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn outdated_status(installed_v: Option<&str>, available_v: Option<&str>) -> OutdatedStatus {
    match (installed_v, available_v) {
        (None, Some(_)) => OutdatedStatus::Untracked,
        (Some(installed), Some(available)) if installed != available => OutdatedStatus::Outdated,
        (None, None) => OutdatedStatus::Outdated,
        _ => OutdatedStatus::Changed,
    }
}

/// Determine whether an installed artifact has been locally modified since
/// installation. Returns `false` if there is no lock entry or the file is not
/// present on disk.
fn check_locally_modified(
    lock_entry: Option<&crate::types::LockEntry>,
    kind: ArtifactKind,
    name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<bool> {
    let Some(entry) = lock_entry else {
        return Ok(false);
    };
    let install_path = ctx.paths.require_installed_artifact_path(kind, name, scope)?;
    if !ctx.fs.exists(&install_path) {
        return Ok(false);
    }
    Ok(checksum::is_locally_modified(&install_path, kind, entry, ctx.fs)?)
}

fn collect_outdated_for_scope_with(
    kind: ArtifactKind,
    scope: InstallScope,
    lock: &LockFile,
    source_artifacts: &BTreeMap<String, Vec<SourceArtifactInfo>>,
    rows: &mut Vec<OutdatedRow>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let pairs =
        config::match_installed_to_sources(kind, scope, lock, source_artifacts, ctx.fs, ctx.paths)?;
    let names: Vec<&str> = pairs.iter().map(|(ia, _)| ia.name.as_str()).collect();
    let modifications = compute_modification_status(kind, scope, &names, lock, ctx)?;
    rows.extend(compare_versions(kind, scope, pairs, &modifications));
    Ok(())
}

fn compute_modification_status(
    kind: ArtifactKind,
    scope: InstallScope,
    names: &[&str],
    lock: &LockFile,
    ctx: &AppContext<'_>,
) -> Result<HashMap<String, bool>> {
    names
        .iter()
        .map(|&name| {
            let lock_entry = lock.packages.get(name);
            let modified = check_locally_modified(lock_entry, kind, name, scope, ctx)?;
            Ok((name.to_string(), modified))
        })
        .collect()
}

fn compare_versions(
    kind: ArtifactKind,
    scope: InstallScope,
    pairs: Vec<InstalledWithSources<'_, SourceArtifactInfo>>,
    modifications: &HashMap<String, bool>,
) -> Vec<OutdatedRow> {
    let mut rows = Vec::new();

    for (ia, source_infos) in pairs {
        let Some(source_infos) = source_infos else {
            continue;
        };

        let installed_v = ia.installed_version.clone();
        let locally_modified = modifications.get(&ia.name).copied().unwrap_or(false);

        for source_info in source_infos {
            let available_v = source_info.version.clone();

            if !source_outdated(
                ia.lock_entry,
                &source_info.checksum,
                source_info.version.as_deref(),
            ) {
                continue;
            }

            rows.push(OutdatedRow {
                name: ia.name.clone(),
                kind,
                scope,
                installed_version: installed_v.clone(),
                available_version: available_v.clone(),
                source: source_info.source_name.clone(),
                status: outdated_status(installed_v.as_deref(), available_v.as_deref()),
                locally_modified,
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
    use crate::lockfile;
    use crate::test_support::{
        TestContext, agent_content, install_agent_on_disk, make_lock_entry_with_checksum,
        save_lock_with_entry, setup_empty_sources, setup_source_with_versioned_agent,
        setup_sources, versioned_agent_content,
    };
    use crate::types::{ArtifactKind, InstallScope, InstalledArtifact, LockFile};
    use std::collections::{BTreeMap, HashMap};

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

        let rows =
            compare_versions(ArtifactKind::Agent, InstallScope::Global, pairs, &modifications);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "my-agent");
        assert_eq!(rows[0].scope, InstallScope::Global);
        assert_eq!(rows[0].available_version.as_deref(), Some("2.0.0"));
        assert_eq!(rows[0].status, OutdatedStatus::Outdated);
        assert!(!rows[0].locally_modified);
    }

    #[test]
    fn compare_versions_carries_locally_modified_as_separate_state() {
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

        let rows =
            compare_versions(ArtifactKind::Agent, InstallScope::Global, pairs, &modifications);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].locally_modified);
        assert_eq!(rows[0].status, OutdatedStatus::Outdated);
    }

    #[test]
    fn compare_versions_skips_pairs_without_source_infos() {
        let ia = InstalledArtifact {
            name: "orphan".to_string(),
            lock_entry: None,
            installed_version: None,
        };
        let pairs: Vec<_> = vec![(ia, None)];
        let modifications = HashMap::new();

        let rows =
            compare_versions(ArtifactKind::Agent, InstallScope::Global, pairs, &modifications);
        assert!(rows.is_empty(), "orphan with no source should produce no rows");
    }

    #[test]
    fn outdated_status_distinguishes_untracked_outdated_and_changed() {
        assert_eq!(outdated_status(None, Some("1.0.0")), OutdatedStatus::Untracked);
        assert_eq!(outdated_status(Some("1.0.0"), Some("2.0.0")), OutdatedStatus::Outdated);
        assert_eq!(outdated_status(Some("1.0.0"), Some("1.0.0")), OutdatedStatus::Changed);
        assert_eq!(outdated_status(None, None), OutdatedStatus::Outdated);
    }

    #[test]
    fn gather_outdated_outdated_artifact_appears_in_rows() {
        let t = TestContext::new();

        setup_source_with_versioned_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "2.0.0",
        );

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "my-agent.md",
                "sha256:old",
            ),
            InstallScope::Global,
        );

        let report = outdated(&t.ctx()).unwrap();

        assert_eq!(report.0.len(), 1, "expected one outdated artifact");
        assert_eq!(report.0[0].name, "my-agent");
        assert_eq!(report.0[0].installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(report.0[0].available_version.as_deref(), Some("2.0.0"));
        assert_eq!(report.0[0].source, "guidelines");
        assert_eq!(report.0[0].status, OutdatedStatus::Outdated);
    }

    #[test]
    fn gather_outdated_up_to_date_returns_empty() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);

        let report = outdated(&t.ctx()).unwrap();

        assert!(report.0.is_empty(), "expected no rows when everything is up to date");
    }

    #[test]
    fn gather_outdated_untracked_artifact_uses_null_installed_version_semantics() {
        let t = TestContext::new();

        setup_source_with_versioned_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "1.0.0",
        );

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            InstallScope::Global,
        );
        lockfile::save(
            &LockFile {
                version: 1,
                packages: BTreeMap::new(),
            },
            InstallScope::Global,
            &t.fs,
            &t.paths,
        )
        .unwrap();

        let report = outdated(&t.ctx()).unwrap();

        assert_eq!(report.0.len(), 1, "untracked artifact should appear");
        assert_eq!(report.0[0].name, "my-agent");
        assert_eq!(report.0[0].installed_version, None);
        assert_eq!(report.0[0].status, OutdatedStatus::Untracked);
        assert!(!report.0[0].locally_modified);
    }

    #[test]
    fn gather_outdated_locally_modified_sets_flag() {
        let t = TestContext::new();

        setup_source_with_versioned_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
            "2.0.0",
        );

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            InstallScope::Global,
        );

        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "guidelines",
            "my-agent.md",
            "sha256:old",
        );
        entry.installed_checksum = "sha256:different".to_string();
        save_lock_with_entry(&t.fs, &t.paths, "my-agent", entry, InstallScope::Global);

        let report = outdated(&t.ctx()).unwrap();

        assert_eq!(report.0.len(), 1);
        assert!(report.0[0].locally_modified);
    }

    #[test]
    fn outdated_shows_rows_from_both_sources_when_artifact_in_two() {
        let t = TestContext::new();

        setup_sources(
            &t.fs,
            &t.paths,
            &[
                ("guidelines", "/sources/guidelines"),
                ("marketplace", "/sources/marketplace"),
            ],
        );
        t.fs.add_file(
            "/sources/guidelines/agents/my-agent.md",
            versioned_agent_content("my-agent", "A test agent", "2.0.0"),
        );
        t.fs.add_file(
            "/sources/marketplace/agents/my-agent.md",
            versioned_agent_content("my-agent", "A test agent", "3.0.0"),
        );

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "A test agent"),
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "guidelines",
                "my-agent.md",
                "sha256:old",
            ),
            InstallScope::Global,
        );

        let report = outdated(&t.ctx()).unwrap();

        assert_eq!(report.0.len(), 2, "should show outdated row for each source");

        let source_names: Vec<&str> = report.0.iter().map(|r| r.source.as_str()).collect();
        assert!(source_names.contains(&"guidelines"));
        assert!(source_names.contains(&"marketplace"));

        let guidelines_row = report.0.iter().find(|r| r.source == "guidelines").unwrap();
        let marketplace_row = report.0.iter().find(|r| r.source == "marketplace").unwrap();
        assert_eq!(guidelines_row.available_version.as_deref(), Some("2.0.0"));
        assert_eq!(marketplace_row.available_version.as_deref(), Some("3.0.0"));
    }
}
