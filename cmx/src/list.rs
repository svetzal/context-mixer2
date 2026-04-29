use anyhow::Result;
use std::collections::BTreeMap;

use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::scan;
use crate::source_iter;
use crate::types::{ArtifactKind, LockFile};

pub struct Row {
    pub name: String,
    pub installed: String,
    pub source: String,
    pub available: String,
    pub status: &'static str,
}

pub struct ListKindOutput {
    pub kind: ArtifactKind,
    pub global_rows: Vec<Row>,
    pub local_rows: Vec<Row>,
}

pub struct ListOutput {
    pub global_agents: Vec<Row>,
    pub local_agents: Vec<Row>,
    pub global_skills: Vec<Row>,
    pub local_skills: Vec<Row>,
}

fn status_indicator(installed: &str, available: &str, deprecated: bool) -> &'static str {
    if deprecated {
        return "⛔";
    }
    match (installed, available) {
        // not installed from this source / unmanaged / no source version
        ("", _) | ("-" | _, "-") => " ",
        ("-", _) => "⚠️",         // installed but no version tracked
        (i, a) if i == a => "✅", // up to date
        _ => "⚠️",                // behind
    }
}

pub fn list_kind_with(kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<ListKindOutput> {
    let source_versions = build_source_versions_with(kind, ctx)?;
    let (global_lock, local_lock) = lockfile::load_both_with(ctx.fs, ctx.paths)?;
    let global_rows = build_rows_with(kind, false, &global_lock, &source_versions, ctx)?;
    let local_rows = build_rows_with(kind, true, &local_lock, &source_versions, ctx)?;
    Ok(ListKindOutput {
        kind,
        global_rows,
        local_rows,
    })
}

pub fn list_all_with(ctx: &AppContext<'_>) -> Result<ListOutput> {
    let (global_lock, local_lock) = lockfile::load_both_with(ctx.fs, ctx.paths)?;
    let mut output = ListOutput {
        global_agents: Vec::new(),
        local_agents: Vec::new(),
        global_skills: Vec::new(),
        local_skills: Vec::new(),
    };

    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        let source_versions = build_source_versions_with(kind, ctx)?;
        let global = build_rows_with(kind, false, &global_lock, &source_versions, ctx)?;
        let local = build_rows_with(kind, true, &local_lock, &source_versions, ctx)?;
        match kind {
            ArtifactKind::Agent => {
                output.global_agents = global;
                output.local_agents = local;
            }
            ArtifactKind::Skill => {
                output.global_skills = global;
                output.local_skills = local;
            }
        }
    }

    Ok(output)
}

fn build_rows_with(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_versions: &BTreeMap<String, Vec<SourceInfo>>,
    ctx: &AppContext<'_>,
) -> Result<Vec<Row>> {
    let names = config::installed_names_with(kind, local, ctx.fs, ctx.paths)?;
    let mut rows = Vec::new();

    for name in names {
        let lock_entry = lock.packages.get(&name);
        let source_infos = source_versions.get(&name);

        let installed = lock_entry
            .and_then(|e| e.version.as_deref())
            .map(str::to_string)
            .or_else(|| read_installed_version(kind, &name, local, ctx))
            .unwrap_or_else(|| "-".to_string());

        if let Some(infos) = source_infos {
            // Emit one row per source that provides this artifact.
            // Only show the installed version on the row matching the
            // source from which the artifact was actually installed.
            for info in infos {
                // No lock entry → show installed on all rows
                let is_install_source =
                    lock_entry.is_none_or(|e| e.source.repo == info.source_name);
                let row_installed = if is_install_source {
                    installed.clone()
                } else {
                    String::new()
                };
                let source = {
                    let path = lock_entry.map_or("", |e| e.source.path.as_str());
                    format_source(&info.source_name, path)
                };
                let status = status_indicator(&row_installed, &info.version, info.deprecated);
                rows.push(Row {
                    name: name.clone(),
                    installed: row_installed,
                    source,
                    available: info.version.clone(),
                    status,
                });
            }
        } else {
            // No source provides this artifact — fall back to lockfile info
            let source_name = lock_entry.map_or_else(|| "-".to_string(), |e| e.source.repo.clone());
            let source = {
                let path = lock_entry.map_or("", |e| e.source.path.as_str());
                format_source(&source_name, path)
            };
            let status = status_indicator(&installed, "-", false);
            rows.push(Row {
                name,
                installed,
                source,
                available: "-".to_string(),
                status,
            });
        }
    }

    Ok(rows)
}

struct SourceInfo {
    source_name: String,
    version: String,
    deprecated: bool,
}

/// Format the Source column to include provenance path when available.
fn format_source(repo: &str, path: &str) -> String {
    if path.is_empty() || repo == "-" {
        repo.to_string()
    } else {
        format!("{repo} ({path})")
    }
}

fn build_source_versions_with(
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<BTreeMap<String, Vec<SourceInfo>>> {
    let mut versions: BTreeMap<String, Vec<SourceInfo>> = BTreeMap::new();

    for sa in source_iter::all_artifacts(ctx)? {
        if sa.artifact.kind == kind {
            let version = sa.artifact.version.as_deref().unwrap_or("-").to_string();
            let deprecated = sa.artifact.is_deprecated();
            versions.entry(sa.artifact.name).or_default().push(SourceInfo {
                source_name: sa.source_name,
                version,
                deprecated,
            });
        }
    }

    Ok(versions)
}

/// Read the version from an installed artifact's file on disk.
fn read_installed_version(
    kind: ArtifactKind,
    name: &str,
    local: bool,
    ctx: &AppContext<'_>,
) -> Option<String> {
    let dir = ctx.paths.install_dir(kind, local);
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
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{
        install_agent_on_disk, make_ctx, make_lock_entry_versioned, setup_empty_sources,
        setup_source_with_agent, setup_source_with_skill, setup_sources, test_paths,
        versioned_agent_content, versioned_skill_content,
    };
    use crate::types::{ArtifactKind, LockFile};
    use chrono::Utc;
    use std::collections::BTreeMap;

    // --- status_indicator ---

    #[test]
    fn status_indicator_deprecated_always_stops() {
        assert_eq!(status_indicator("1.0", "1.0", true), "⛔");
        assert_eq!(status_indicator("-", "-", true), "⛔");
        assert_eq!(status_indicator("1.0", "2.0", true), "⛔");
    }

    #[test]
    fn status_indicator_unmanaged_no_versions() {
        // Both are "-" and not deprecated — unmanaged
        assert_eq!(status_indicator("-", "-", false), " ");
    }

    #[test]
    fn status_indicator_installed_no_version_tracked() {
        // installed="-" means no version tracked but artifact is installed
        assert_eq!(status_indicator("-", "1.0", false), "⚠️");
    }

    #[test]
    fn status_indicator_no_source_version() {
        // available="-" means no upstream version to compare
        assert_eq!(status_indicator("1.0", "-", false), " ");
    }

    #[test]
    fn status_indicator_up_to_date() {
        assert_eq!(status_indicator("1.0", "1.0", false), "✅");
    }

    #[test]
    fn status_indicator_behind() {
        assert_eq!(status_indicator("1.0", "2.0", false), "⚠️");
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
        local: bool,
    ) {
        let skill_dir = paths.install_dir(ArtifactKind::Skill, local);
        fs.add_file(
            skill_dir.join(skill_name).join("SKILL.md"),
            versioned_skill_content("A test skill", skill_version),
        );
    }

    #[test]
    fn skill_row_shows_source_provenance_and_both_versions() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has skill at version 2.0.0
        setup_source_with_skill(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-skill",
            "2.0.0",
        );

        // Installed skill at version 1.0.0
        install_skill_dir(&fs, &paths, "my-skill", "1.0.0", false);

        // Lockfile records installed version 1.0.0 from guidelines
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_lock_entry_versioned(ArtifactKind::Skill, "1.0.0", "guidelines", "my-skill"),
        );
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Skill, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Skill, false, &lock, &source_versions, &ctx).unwrap();

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
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has skill at a nested path within the repo
        setup_source_with_skill(
            &fs,
            &paths,
            "marketplace",
            "/sources/marketplace",
            "pdf-tool",
            "1.0.0",
        );

        // Install the skill
        install_skill_dir(&fs, &paths, "pdf-tool", "1.0.0", false);

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
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Skill, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Skill, false, &lock, &source_versions, &ctx).unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row.source, "marketplace (plugins/doc-tools/skills/pdf-tool)",
            "source should show repo name and full provenance path"
        );
    }

    #[test]
    fn skill_row_no_lockfile_shows_repo_name_only() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has skill
        setup_source_with_skill(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-skill",
            "1.0.0",
        );

        // Skill installed on disk but NOT in lockfile
        install_skill_dir(&fs, &paths, "my-skill", "1.0.0", false);

        let lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Skill, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Skill, false, &lock, &source_versions, &ctx).unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.source, "guidelines", "no lockfile entry means repo name only");
        assert_eq!(row.installed, "1.0.0", "version read from installed SKILL.md");
        assert_eq!(row.available, "1.0.0", "source version still shown");
    }

    #[test]
    fn skill_row_source_removed_shows_lockfile_provenance() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // No source registered — empty sources.json
        setup_empty_sources(&fs, &paths);

        // Skill installed on disk
        install_skill_dir(&fs, &paths, "my-skill", "1.0.0", false);

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
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Skill, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Skill, false, &lock, &source_versions, &ctx).unwrap();

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
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source with agent at version 2.0.0
        setup_source_with_agent(&fs, &paths, "guidelines", "/sources/guidelines", "my-agent");

        // Install agent on disk
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &crate::test_support::agent_content("my-agent", "A test agent"),
            false,
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
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Agent, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Agent, false, &lock, &source_versions, &ctx).unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.source.contains("guidelines"), "agent row should show source repo");
        assert!(row.source.contains("my-agent.md"), "agent row should show provenance path");
    }

    // --- multi-source: same artifact in multiple sources ---

    #[test]
    fn agent_in_two_sources_produces_two_rows() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Register two sources that both contain the same agent
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

        // Install agent on disk
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            &versioned_agent_content("my-agent", "A test agent", "1.0.0"),
            false,
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
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Agent, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Agent, false, &lock, &source_versions, &ctx).unwrap();

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
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Register two sources with the same skill
        setup_sources(
            &fs,
            &paths,
            &[
                ("guidelines", "/sources/guidelines"),
                ("marketplace", "/sources/marketplace"),
            ],
        );
        fs.add_file(
            "/sources/guidelines/my-skill/SKILL.md",
            versioned_skill_content("A test skill", "1.0.0"),
        );
        fs.add_file(
            "/sources/marketplace/my-skill/SKILL.md",
            versioned_skill_content("A test skill", "2.0.0"),
        );

        // Install skill on disk
        install_skill_dir(&fs, &paths, "my-skill", "1.0.0", false);

        // Lockfile records installed from guidelines
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_lock_entry_versioned(ArtifactKind::Skill, "1.0.0", "guidelines", "my-skill"),
        );
        crate::lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let source_versions = build_source_versions_with(ArtifactKind::Skill, &ctx).unwrap();
        let rows =
            build_rows_with(ArtifactKind::Skill, false, &lock, &source_versions, &ctx).unwrap();

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
