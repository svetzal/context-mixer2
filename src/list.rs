use anyhow::Result;
use std::collections::BTreeMap;

use crate::config;
use crate::lockfile;
use crate::scan;
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

pub fn list_kind(kind: ArtifactKind) -> Result<()> {
    let source_versions = build_source_versions(kind)?;
    let global_lock = lockfile::load(false)?;
    let local_lock = lockfile::load(true)?;
    let global = build_rows(kind, false, &global_lock, &source_versions)?;
    let local = build_rows(kind, true, &local_lock, &source_versions)?;

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

pub fn list_all() -> Result<()> {
    let agent_versions = build_source_versions(ArtifactKind::Agent)?;
    let skill_versions = build_source_versions(ArtifactKind::Skill)?;
    let global_lock = lockfile::load(false)?;
    let local_lock = lockfile::load(true)?;

    let global_agents = build_rows(ArtifactKind::Agent, false, &global_lock, &agent_versions)?;
    let local_agents = build_rows(ArtifactKind::Agent, true, &local_lock, &agent_versions)?;
    let global_skills = build_rows(ArtifactKind::Skill, false, &global_lock, &skill_versions)?;
    let local_skills = build_rows(ArtifactKind::Skill, true, &local_lock, &skill_versions)?;

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

fn build_rows(
    kind: ArtifactKind,
    local: bool,
    lock: &LockFile,
    source_versions: &BTreeMap<String, SourceInfo>,
) -> Result<Vec<Row>> {
    let names = config::installed_names(kind, local)?;
    let mut rows = Vec::new();

    for name in names {
        let lock_entry = lock.packages.get(&name);
        let source_info = source_versions.get(&name);

        let installed = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-").to_string();

        let (source, available, deprecated) = match source_info {
            Some(info) => (info.source_name.clone(), info.version.clone(), info.deprecated),
            None => {
                let src =
                    lock_entry.map(|e| e.source.repo.clone()).unwrap_or_else(|| "-".to_string());
                (src, "-".to_string(), false)
            }
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

fn build_source_versions(kind: ArtifactKind) -> Result<BTreeMap<String, SourceInfo>> {
    let mut versions = BTreeMap::new();
    let sources = config::load_sources()?;

    for (source_name, entry) in &sources.sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in artifacts {
                if artifact.artifact_kind() == kind {
                    let version = artifact.version().unwrap_or("-").to_string();
                    let deprecated = artifact.is_deprecated();
                    versions.insert(
                        artifact.name().to_string(),
                        SourceInfo {
                            source_name: source_name.clone(),
                            version,
                            deprecated,
                        },
                    );
                }
            }
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
