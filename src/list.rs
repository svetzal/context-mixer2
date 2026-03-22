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

        let (source, available, deprecated) = if let Some(info) = source_info {
            (info.source_name.clone(), info.version.clone(), info.deprecated)
        } else {
            let src = lock_entry.map_or_else(|| "-".to_string(), |e| e.source.repo.clone());
            (src, "-".to_string(), false)
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
}
