use anyhow::Result;
use std::collections::{BTreeMap, HashMap};

use crate::config;
use crate::config::InstalledWithSources;
use crate::context::{AppContext, LoadedState};
use crate::scan;
use crate::source_iter;
use crate::source_iter::SourceArtifactInfo;
use crate::types::{
    ArtifactKind, InstallScope, InstalledArtifact, LockEntry, LockFile, display_version,
};

pub struct Row {
    pub name: String,
    pub installed: String,
    pub source: String,
    pub available: String,
    pub status: &'static str,
}

pub struct ListKindOutput {
    pub kind: ArtifactKind,
    pub rows: BTreeMap<InstallScope, Vec<Row>>,
}

pub struct ListOutput {
    pub agents: BTreeMap<InstallScope, Vec<Row>>,
    pub skills: BTreeMap<InstallScope, Vec<Row>>,
}

fn status_indicator(
    installed: Option<&str>,
    available: Option<&str>,
    deprecated: bool,
) -> &'static str {
    if deprecated {
        return "⛔";
    }
    match (installed, available) {
        (None | Some(_), None) => " ",
        (Some(i), Some(a)) if i == a => "✅",
        _ => "⚠️",
    }
}

pub fn list_kind_with(kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<ListKindOutput> {
    let loaded = LoadedState::load(ctx)?;
    let source_versions = source_iter::scan_all_with_checksums(&loaded.sources.sources, ctx.fs)?;
    let mut rows = BTreeMap::new();
    for (scope, lock) in loaded.scopes() {
        rows.insert(scope, build_rows_with(kind, scope, lock, &source_versions, ctx)?);
    }
    Ok(ListKindOutput { kind, rows })
}

pub fn list_all_with(ctx: &AppContext<'_>) -> Result<ListOutput> {
    let loaded = LoadedState::load(ctx)?;
    let source_versions = source_iter::scan_all_with_checksums(&loaded.sources.sources, ctx.fs)?;
    let mut agents = BTreeMap::new();
    let mut skills = BTreeMap::new();
    for (scope, lock) in loaded.scopes() {
        agents.insert(
            scope,
            build_rows_with(ArtifactKind::Agent, scope, lock, &source_versions, ctx)?,
        );
        skills.insert(
            scope,
            build_rows_with(ArtifactKind::Skill, scope, lock, &source_versions, ctx)?,
        );
    }
    Ok(ListOutput { agents, skills })
}

fn build_rows_with(
    kind: ArtifactKind,
    scope: InstallScope,
    lock: &LockFile,
    source_versions: &BTreeMap<String, Vec<SourceArtifactInfo>>,
    ctx: &AppContext<'_>,
) -> Result<Vec<Row>> {
    let pairs =
        config::match_installed_to_sources(kind, scope, lock, source_versions, ctx.fs, ctx.paths)?;
    let names: Vec<&str> = pairs.iter().map(|(ia, _)| ia.name.as_str()).collect();
    let installed_versions = load_installed_versions(kind, scope, &names, ctx);
    Ok(assemble_rows(pairs, &installed_versions))
}

/// Pre-load installed artifact versions from disk for a batch of artifact names.
fn load_installed_versions(
    kind: ArtifactKind,
    scope: InstallScope,
    names: &[&str],
    ctx: &AppContext<'_>,
) -> HashMap<String, Option<String>> {
    names
        .iter()
        .map(|&name| (name.to_string(), read_installed_version(kind, name, scope, ctx)))
        .collect()
}

/// Pure row assembly — no filesystem access. Accepts pre-computed pairs and
/// pre-loaded installed versions.
fn assemble_rows(
    pairs: Vec<InstalledWithSources<'_, SourceArtifactInfo>>,
    installed_versions: &HashMap<String, Option<String>>,
) -> Vec<Row> {
    let mut rows = Vec::new();

    for (ia, source_infos) in pairs {
        let lock_entry = ia.lock_entry;

        let installed: Option<String> = ia
            .installed_version
            .clone()
            .or_else(|| installed_versions.get(&ia.name).and_then(Clone::clone));

        if let Some(infos) = source_infos {
            rows.extend(build_rows_from_sources(&ia, lock_entry, installed.as_deref(), infos));
        } else {
            rows.push(build_orphan_row(&ia, lock_entry, installed.as_deref()));
        }
    }

    rows
}

/// Build rows for an artifact that has one or more source entries.
///
/// Emits one row per source. Only the row for the source the artifact was
/// actually installed from shows the installed version.
fn build_rows_from_sources(
    ia: &InstalledArtifact<'_>,
    lock_entry: Option<&LockEntry>,
    installed: Option<&str>,
    infos: &[SourceArtifactInfo],
) -> Vec<Row> {
    let mut rows = Vec::new();
    for info in infos {
        // No lock entry → show installed on all rows
        let is_install_source = lock_entry.is_none_or(|e| e.source.repo == info.source_name);
        let row_installed = if is_install_source {
            display_version(installed).to_string()
        } else {
            String::new()
        };
        let source = {
            let path = lock_entry.map_or("", |e| e.source.path.as_str());
            format_source(&info.source_name, path)
        };
        let status = status_indicator(installed, info.version.as_deref(), info.deprecated);
        rows.push(Row {
            name: ia.name.clone(),
            installed: row_installed,
            source,
            available: display_version(info.version.as_deref()).to_string(),
            status,
        });
    }
    rows
}

/// Build a single row for an artifact with no matching source (orphan).
///
/// Falls back to lockfile provenance for the source column.
fn build_orphan_row(
    ia: &InstalledArtifact<'_>,
    lock_entry: Option<&LockEntry>,
    installed: Option<&str>,
) -> Row {
    let source_name = lock_entry.map_or_else(|| "-".to_string(), |e| e.source.repo.clone());
    let source = {
        let path = lock_entry.map_or("", |e| e.source.path.as_str());
        format_source(&source_name, path)
    };
    let status = status_indicator(installed, None, false);
    Row {
        name: ia.name.clone(),
        installed: display_version(installed).to_string(),
        source,
        available: display_version(None).to_string(),
        status,
    }
}

/// Format the Source column to include provenance path when available.
fn format_source(repo: &str, path: &str) -> String {
    if path.is_empty() || repo == "-" {
        repo.to_string()
    } else {
        format!("{repo} ({path})")
    }
}

/// Read the version from an installed artifact's file on disk.
fn read_installed_version(
    kind: ArtifactKind,
    name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Option<String> {
    let dir = ctx.paths.install_dir(kind, scope);
    let file_path = kind.content_path(&kind.installed_path(name, &dir));
    let content = ctx.fs.read_to_string(&file_path).ok()?;
    scan::extract_version_from_content(&content)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{
        TestContext, install_agent_on_disk, make_lock_entry_versioned, setup_empty_sources,
        setup_source_with_agent, setup_source_with_skill, setup_sources, versioned_agent_content,
        versioned_skill_content,
    };
    use crate::types::{ArtifactKind, InstallScope, LockFile};
    use std::collections::BTreeMap;

    // --- assemble_rows (pure, no gateway fakes needed) ---

    #[test]
    fn assemble_rows_builds_row_from_source_with_pre_loaded_versions() {
        let mut packages = BTreeMap::new();
        packages.insert(
            "my-agent".to_string(),
            make_lock_entry_versioned(
                ArtifactKind::Agent,
                "1.0.0",
                "guidelines",
                "agents/my-agent.md",
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
        let source_infos = vec![SourceArtifactInfo {
            source_name: "guidelines".to_string(),
            version: Some("2.0.0".to_string()),
            checksum: "sha256:abc".to_string(),
            deprecated: false,
        }];
        let pairs = vec![(ia, Some(&source_infos))];
        let installed_versions =
            HashMap::from([("my-agent".to_string(), Some("1.0.0".to_string()))]);

        let rows = assemble_rows(pairs, &installed_versions);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "my-agent");
        assert_eq!(rows[0].installed, "1.0.0");
        assert_eq!(rows[0].available, "2.0.0");
    }

    #[test]
    fn assemble_rows_falls_back_to_installed_versions_map_when_lock_version_absent() {
        let lock = LockFile::default();
        let ia = InstalledArtifact {
            name: "unversioned-agent".to_string(),
            lock_entry: None,
            installed_version: None,
        };
        let source_infos = vec![SourceArtifactInfo {
            source_name: "guidelines".to_string(),
            version: Some("1.0.0".to_string()),
            checksum: "sha256:abc".to_string(),
            deprecated: false,
        }];
        let pairs = vec![(ia, Some(&source_infos))];
        let installed_versions =
            HashMap::from([("unversioned-agent".to_string(), Some("0.9.0".to_string()))]);

        let rows = assemble_rows(pairs, &installed_versions);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].installed, "0.9.0");
        let _ = lock; // ensure lock outlives
    }

    // --- status_indicator ---

    #[test]
    fn status_indicator_deprecated_always_stops() {
        assert_eq!(status_indicator(Some("1.0"), Some("1.0"), true), "⛔");
        assert_eq!(status_indicator(None, None, true), "⛔");
        assert_eq!(status_indicator(Some("1.0"), Some("2.0"), true), "⛔");
    }

    #[test]
    fn status_indicator_unmanaged_no_versions() {
        // Both None and not deprecated — unmanaged
        assert_eq!(status_indicator(None, None, false), " ");
    }

    #[test]
    fn status_indicator_installed_no_version_tracked() {
        // installed=None means no version tracked but artifact is installed
        assert_eq!(status_indicator(None, Some("1.0"), false), "⚠️");
    }

    #[test]
    fn status_indicator_no_source_version() {
        // available=None means no upstream version to compare
        assert_eq!(status_indicator(Some("1.0"), None, false), " ");
    }

    #[test]
    fn status_indicator_up_to_date() {
        assert_eq!(status_indicator(Some("1.0"), Some("1.0"), false), "✅");
    }

    #[test]
    fn status_indicator_behind() {
        assert_eq!(status_indicator(Some("1.0"), Some("2.0"), false), "⚠️");
    }

    // --- format_source ---

    #[test]
    fn format_source_with_path_shows_provenance() {
        assert_eq!(format_source("guidelines", "skills/my-skill"), "guidelines (skills/my-skill)");
    }

    #[test]
    fn format_source_empty_path_shows_repo_only() {
        assert_eq!(format_source("guidelines", ""), "guidelines");
    }

    #[test]
    fn format_source_dash_repo_stays_dash() {
        assert_eq!(format_source("-", ""), "-");
        assert_eq!(format_source("-", "some/path"), "-");
    }

    // --- build_rows_with: skill with distinct installed vs source version ---

    fn install_skill_dir(
        fs: &FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        skill_name: &str,
        skill_version: &str,
        scope: InstallScope,
    ) {
        let skill_dir = paths.install_dir(ArtifactKind::Skill, scope);
        fs.add_file(
            skill_dir.join(skill_name).join("SKILL.md"),
            versioned_skill_content("A test skill", skill_version),
        );
    }

    #[test]
    fn skill_row_shows_source_provenance_and_both_versions() {
        let t = TestContext::new();

        // Source has skill at version 2.0.0
        setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "my-skill",
            "2.0.0",
        );

        // Installed skill at version 1.0.0
        install_skill_dir(&t.fs, &t.paths, "my-skill", "1.0.0", InstallScope::Global);

        // Lockfile records installed version 1.0.0 from guidelines
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_lock_entry_versioned(ArtifactKind::Skill, "1.0.0", "guidelines", "my-skill"),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Skill,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.name, "my-skill");
        assert_eq!(row.installed, "1.0.0", "installed version from lockfile");
        assert_eq!(row.available, "2.0.0", "available version from source");
        assert!(
            row.source.contains("guidelines"),
            "source must show repo name, got: {}",
            row.source
        );
        assert!(
            row.source.contains("my-skill"),
            "source must show provenance path, got: {}",
            row.source
        );
    }

    #[test]
    fn skill_row_source_includes_full_provenance_path() {
        let t = TestContext::new();

        // Source has skill at a nested path within the repo
        setup_source_with_skill(
            &t.fs,
            &t.paths,
            "marketplace",
            "/sources/marketplace",
            "pdf-tool",
            "1.0.0",
        );

        // Install the skill
        install_skill_dir(&t.fs, &t.paths, "pdf-tool", "1.0.0", InstallScope::Global);

        // Lockfile records source path as a nested marketplace location
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "pdf-tool".to_string(),
            make_lock_entry_versioned(
                ArtifactKind::Skill,
                "1.0.0",
                "marketplace",
                "plugins/doc-tools/skills/pdf-tool",
            ),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Skill,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row.source, "marketplace (plugins/doc-tools/skills/pdf-tool)",
            "source should show repo name and full provenance path"
        );
    }

    #[test]
    fn skill_row_no_lockfile_shows_repo_name_only() {
        let t = TestContext::new();

        // Source has skill
        setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/sources/guidelines",
            "my-skill",
            "1.0.0",
        );

        // Skill installed on disk but NOT in lockfile
        install_skill_dir(&t.fs, &t.paths, "my-skill", "1.0.0", InstallScope::Global);

        let lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Skill,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.source, "guidelines", "no lockfile entry means repo name only");
        assert_eq!(row.installed, "1.0.0", "version read from installed SKILL.md");
        assert_eq!(row.available, "1.0.0", "source version still shown");
    }

    #[test]
    fn skill_row_source_removed_shows_lockfile_provenance() {
        let t = TestContext::new();

        // No source registered — empty sources.json
        setup_empty_sources(&t.fs, &t.paths);

        // Skill installed on disk
        install_skill_dir(&t.fs, &t.paths, "my-skill", "1.0.0", InstallScope::Global);

        // Lockfile still has the provenance
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_lock_entry_versioned(
                ArtifactKind::Skill,
                "1.0.0",
                "guidelines",
                "skills/my-skill",
            ),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Skill,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row.source, "guidelines (skills/my-skill)",
            "source removed: lockfile provenance (repo + path) should still show"
        );
        assert_eq!(row.installed, "1.0.0");
        assert_eq!(row.available, "-", "no source means no available version");
    }

    #[test]
    fn agent_row_also_shows_source_provenance() {
        let t = TestContext::new();

        // Source with agent at version 2.0.0
        setup_source_with_agent(&t.fs, &t.paths, "guidelines", "/sources/guidelines", "my-agent");

        // Install agent on disk
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &crate::test_support::agent_content("my-agent", "A test agent"),
            InstallScope::Global,
        );

        // Lockfile records installed version 1.0.0
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "guidelines", "my-agent.md"),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Agent,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.source.contains("guidelines"), "agent row should show source repo");
        assert!(row.source.contains("my-agent.md"), "agent row should show provenance path");
    }

    // --- multi-source: same artifact in multiple sources ---

    #[test]
    fn agent_in_two_sources_produces_two_rows() {
        let t = TestContext::new();

        // Register two sources that both contain the same agent
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

        // Install agent on disk
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &versioned_agent_content("my-agent", "A test agent", "1.0.0"),
            InstallScope::Global,
        );

        // Lockfile records installed from guidelines
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "guidelines", "my-agent.md"),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Agent,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 2, "should have one row per source");

        let sources: Vec<&str> = rows.iter().map(|r| r.source.as_str()).collect();
        assert!(sources.iter().any(|s| s.contains("guidelines")), "should have a guidelines row");
        assert!(
            sources.iter().any(|s| s.contains("marketplace")),
            "should have a marketplace row"
        );

        // Only the row from the install source shows the installed version
        let guidelines_row = rows.iter().find(|r| r.source.contains("guidelines")).unwrap();
        let marketplace_row = rows.iter().find(|r| r.source.contains("marketplace")).unwrap();
        assert_eq!(guidelines_row.installed, "1.0.0");
        assert_eq!(marketplace_row.installed, "", "non-install source should be blank");

        // Each row shows the available version from its own source
        assert_eq!(guidelines_row.available, "2.0.0");
        assert_eq!(marketplace_row.available, "3.0.0");
    }

    #[test]
    fn skill_in_two_sources_produces_two_rows() {
        let t = TestContext::new();

        // Register two sources with the same skill
        setup_sources(
            &t.fs,
            &t.paths,
            &[
                ("guidelines", "/sources/guidelines"),
                ("marketplace", "/sources/marketplace"),
            ],
        );
        t.fs.add_file(
            "/sources/guidelines/my-skill/SKILL.md",
            versioned_skill_content("A test skill", "1.0.0"),
        );
        t.fs.add_file(
            "/sources/marketplace/my-skill/SKILL.md",
            versioned_skill_content("A test skill", "2.0.0"),
        );

        // Install skill on disk
        install_skill_dir(&t.fs, &t.paths, "my-skill", "1.0.0", InstallScope::Global);

        // Lockfile records installed from guidelines
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_lock_entry_versioned(ArtifactKind::Skill, "1.0.0", "guidelines", "my-skill"),
        );
        crate::lockfile::save_with(&lock, InstallScope::Global, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let source_versions = source_iter::all_with_checksums(&ctx).unwrap();
        let rows = build_rows_with(
            ArtifactKind::Skill,
            InstallScope::Global,
            &lock,
            &source_versions,
            &ctx,
        )
        .unwrap();

        assert_eq!(rows.len(), 2, "should have one row per source");

        let guidelines_row = rows.iter().find(|r| r.source.contains("guidelines")).unwrap();
        let marketplace_row = rows.iter().find(|r| r.source.contains("marketplace")).unwrap();

        // Only the install source row shows the installed version
        assert_eq!(guidelines_row.installed, "1.0.0");
        assert_eq!(marketplace_row.installed, "", "non-install source should be blank");

        assert_eq!(guidelines_row.available, "1.0.0");
        assert_eq!(marketplace_row.available, "2.0.0");
    }
}
