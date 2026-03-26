use anyhow::Result;
use std::collections::BTreeMap;

use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source_iter;
use crate::types::{ArtifactKind, LockFile};

struct Row {
    name: String,
    installed: String,
    source: String,
    available: String,
    status: &'static str,
}

fn status_indicator(installed: &str, available: &str, deprecated: bool) -> &'static str {
    if deprecated {
        return "⛔";
    }
    match (installed, available) {
        ("-", "-") => " ",        // no source, unmanaged
        ("-", _) => "⚠️",         // installed but no version tracked
        (_, "-") => " ",          // no source version to compare
        (i, a) if i == a => "✅", // up to date
        _ => "⚠️",                // behind
    }
}

pub fn list_kind_with(kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    let source_versions = build_source_versions_with(kind, ctx)?;
    let global_lock = lockfile::load_with(false, ctx.fs, ctx.paths)?;
    let local_lock = lockfile::load_with(true, ctx.fs, ctx.paths)?;
    let global = build_rows_with(kind, false, &global_lock, &source_versions, ctx)?;
    let local = build_rows_with(kind, true, &local_lock, &source_versions, ctx)?;

    if global.is_empty() && local.is_empty() {
        println!("No {kind}s installed.");
        return Ok(());
    }

    if !global.is_empty() {
        println!("Global {kind}s:");
        print_table(&global);
    }

    if !local.is_empty() {
        if !global.is_empty() {
            println!();
        }
        println!("Local {kind}s:");
        print_table(&local);
    }

    Ok(())
}

pub fn list_all_with(ctx: &AppContext<'_>) -> Result<()> {
    let agent_versions = build_source_versions_with(ArtifactKind::Agent, ctx)?;
    let skill_versions = build_source_versions_with(ArtifactKind::Skill, ctx)?;
    let global_lock = lockfile::load_with(false, ctx.fs, ctx.paths)?;
    let local_lock = lockfile::load_with(true, ctx.fs, ctx.paths)?;

    let global_agents =
        build_rows_with(ArtifactKind::Agent, false, &global_lock, &agent_versions, ctx)?;
    let local_agents =
        build_rows_with(ArtifactKind::Agent, true, &local_lock, &agent_versions, ctx)?;
    let global_skills =
        build_rows_with(ArtifactKind::Skill, false, &global_lock, &skill_versions, ctx)?;
    let local_skills =
        build_rows_with(ArtifactKind::Skill, true, &local_lock, &skill_versions, ctx)?;

    if global_agents.is_empty()
        && local_agents.is_empty()
        && global_skills.is_empty()
        && local_skills.is_empty()
    {
        println!("Nothing installed.");
        return Ok(());
    }

    print_section("Global agents", &global_agents);
    print_section("Local agents", &local_agents);
    print_section("Global skills", &global_skills);
    print_section("Local skills", &local_skills);

    Ok(())
}

fn build_rows_with(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_versions: &BTreeMap<String, SourceInfo>,
    ctx: &AppContext<'_>,
) -> Result<Vec<Row>> {
    let names = config::installed_names_with(kind, local, ctx.fs, ctx.paths)?;
    let mut rows = Vec::new();

    for name in names {
        let lock_entry = lock.packages.get(&name);
        let source_info = source_versions.get(&name);

        let installed = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-").to_string();

        let (source_name, available, deprecated) = if let Some(info) = source_info {
            (info.source_name.clone(), info.version.clone(), info.deprecated)
        } else {
            let src = lock_entry.map_or_else(|| "-".to_string(), |e| e.source.repo.clone());
            (src, "-".to_string(), false)
        };

        let source = {
            let path = lock_entry.map_or("", |e| e.source.path.as_str());
            format_source(&source_name, path)
        };

        let status = status_indicator(&installed, &available, deprecated);

        rows.push(Row {
            name,
            installed,
            source,
            available,
            status,
        });
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
) -> Result<BTreeMap<String, SourceInfo>> {
    let mut versions = BTreeMap::new();
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        if sa.artifact.kind == kind {
            let version = sa.artifact.version.as_deref().unwrap_or("-").to_string();
            let deprecated = sa.artifact.is_deprecated();
            versions.insert(
                sa.artifact.name,
                SourceInfo {
                    source_name: sa.source_name,
                    version,
                    deprecated,
                },
            );
        }
    }

    Ok(versions)
}

fn print_table(rows: &[Row]) {
    if rows.is_empty() {
        return;
    }

    let w_name = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let w_inst = rows.iter().map(|r| r.installed.len()).max().unwrap_or(9).max(9);
    let w_src = rows.iter().map(|r| r.source.len()).max().unwrap_or(6).max(6);
    let w_avail = rows.iter().map(|r| r.available.len()).max().unwrap_or(9).max(9);

    println!(
        "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}",
        "Name", "Installed", "Source", "Available",
    );
    println!(
        "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}",
        "-".repeat(w_name),
        "-".repeat(w_inst),
        "-".repeat(w_src),
        "-".repeat(w_avail),
    );

    for row in rows {
        println!(
            "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}  {}",
            row.name, row.installed, row.source, row.available, row.status,
        );
    }
}

fn print_section(label: &str, rows: &[Row]) {
    println!("{label}:");
    if rows.is_empty() {
        println!("  (none)");
    } else {
        print_table(rows);
    }
    println!();
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{make_ctx, test_paths, versioned_skill_content};
    use crate::types::{ArtifactKind, LockEntry, LockSource, SourceEntry, SourceType, SourcesFile};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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

    fn setup_source_with_skill(
        fs: &FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        source_name: &str,
        source_path: &str,
        skill_name: &str,
        skill_version: &str,
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

        fs.add_file(
            format!("{source_path}/{skill_name}/SKILL.md"),
            versioned_skill_content("A test skill", skill_version),
        );
    }

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

    fn make_skill_lock_entry(version: &str, repo: &str, path: &str) -> LockEntry {
        LockEntry {
            artifact_type: ArtifactKind::Skill,
            version: Some(version.to_string()),
            installed_at: "2024-01-01T00:00:00Z".to_string(),
            source: LockSource {
                repo: repo.to_string(),
                path: path.to_string(),
            },
            source_checksum: "sha256:old".to_string(),
            installed_checksum: "sha256:old".to_string(),
        }
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
            make_skill_lock_entry("1.0.0", "guidelines", "my-skill"),
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
            make_skill_lock_entry("1.0.0", "marketplace", "plugins/doc-tools/skills/pdf-tool"),
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
        assert_eq!(row.installed, "-", "no lockfile means no installed version");
        assert_eq!(row.available, "1.0.0", "source version still shown");
    }

    #[test]
    fn skill_row_source_removed_shows_lockfile_provenance() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // No source registered — empty sources.json
        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        // Skill installed on disk
        install_skill_dir(&fs, &paths, "my-skill", "1.0.0", false);

        // Lockfile still has the provenance
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-skill".to_string(),
            make_skill_lock_entry("1.0.0", "guidelines", "skills/my-skill"),
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
        crate::test_support::setup_source_with_agent(
            &fs,
            &paths,
            "guidelines",
            "/sources/guidelines",
            "my-agent",
        );

        // Install agent on disk
        crate::test_support::install_agent_on_disk(
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
}
