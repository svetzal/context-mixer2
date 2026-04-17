use std::path::Path;

use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
use crate::info::ArtifactInfo;
use crate::install::{InstallAllResult, InstallResult, UpdateAllResult};
use crate::list::{ListKindOutput, ListOutput, Row};
use crate::outdated::OutdatedRow;
use crate::search::SearchOutput;
use crate::source::{
    SourceAddResult, SourceBrowseResult, SourceListResult, SourceRemoveResult, SourceUpdateOutput,
};
use crate::table::Table;
use crate::uninstall::UninstallResult;

pub fn print_list_kind_output(output: &ListKindOutput) {
    let kind = output.kind;
    let global = &output.global_rows;
    let local = &output.local_rows;

    if global.is_empty() && local.is_empty() {
        println!("No {kind}s installed.");
        return;
    }

    if !global.is_empty() {
        println!("Global {kind}s:");
        print_table(global);
    }

    if !local.is_empty() {
        if !global.is_empty() {
            println!();
        }
        println!("Local {kind}s:");
        print_table(local);
    }
}

pub fn print_list_all_output(output: &ListOutput) {
    if output.global_agents.is_empty()
        && output.local_agents.is_empty()
        && output.global_skills.is_empty()
        && output.local_skills.is_empty()
    {
        println!("Nothing installed.");
        return;
    }

    print_section("Global agents", &output.global_agents);
    print_section("Local agents", &output.local_agents);
    print_section("Global skills", &output.global_skills);
    print_section("Local skills", &output.local_skills);
}

pub fn print_table(rows: &[Row]) {
    if rows.is_empty() {
        return;
    }

    Table {
        headers: vec!["Name", "Installed", "Source", "Available"],
        padded_cols: 4,
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.installed.clone(),
                    r.source.clone(),
                    r.available.clone(),
                    r.status.to_string(),
                ]
            })
            .collect(),
    }
    .print();
}

pub fn print_section(label: &str, rows: &[Row]) {
    println!("{label}:");
    if rows.is_empty() {
        println!("  (none)");
    } else {
        print_table(rows);
    }
    println!();
}

pub fn print_source_list(result: &SourceListResult) {
    if result.entries.is_empty() {
        println!("No sources registered.");
        println!();
        println!("Add one with: cmx source add <name> <path-or-url>");
        return;
    }

    for entry in &result.entries {
        println!("  {:<28} ({}) {}", entry.name, entry.kind, entry.location);
    }
}

pub fn print_browse_result(result: &SourceBrowseResult) {
    let name = &result.source_name;

    if result.agents.is_empty() && result.skills.is_empty() {
        println!("No agents or skills found in '{name}'.");
        return;
    }

    if !result.agents.is_empty() {
        println!("Agents:");
        for a in &result.agents {
            let v = a.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            println!("  {}{v}{}", a.name, a.deprecation_display);
        }
    }

    if !result.skills.is_empty() {
        if !result.agents.is_empty() {
            println!();
        }
        println!("Skills:");
        for s in &result.skills {
            let v = s.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            println!("  {}{v}{}", s.name, s.deprecation_display);
            for f in &s.files {
                println!("    {f}");
            }
        }
    }
}

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

pub fn print_uninstall_result(result: &UninstallResult) {
    println!("Uninstalled {} ({}) from {} scope.", result.name, result.kind, result.scope);
    if !result.was_tracked {
        println!("  (no lock file entry found — artifact was untracked)");
    }
}

pub fn print_search_results(output: &SearchOutput) {
    let query = &output.query;
    let results = &output.results;

    if results.is_empty() {
        println!("No results for '{query}'.");
        return;
    }

    Table {
        headers: vec!["Name", "Type", "Version", "Source", "Description"],
        padded_cols: 4,
        rows: results
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.kind.clone(),
                    r.version.clone(),
                    r.source.clone(),
                    r.description.clone(),
                ]
            })
            .collect(),
    }
    .print();

    println!();
    println!("{} result(s) found.", results.len());
}

pub fn print_info(info: &ArtifactInfo) {
    println!("Name:        {}", info.name);
    println!("Type:        {}", info.kind);
    println!("Scope:       {}", info.scope);
    println!("Path:        {}", info.path.display());

    if let Some(v) = &info.version {
        println!("Version:     {v}");
    }
    if let Some(at) = &info.installed_at {
        println!("Installed:   {at}");
    }
    if let Some(src) = &info.source_display {
        println!("Source:      {src}");
    }
    if let Some(cs) = &info.source_checksum {
        println!("Source SHA:  {cs}");
    }
    if let Some(cs) = &info.installed_checksum {
        println!("Install SHA: {cs}");
    }
    if info.locally_modified {
        let disk_cs = info.disk_checksum.as_deref().unwrap_or("unknown");
        println!("Disk SHA:    {disk_cs}  (locally modified)");
    }
    if info.untracked {
        println!("Lock entry:  (none — untracked)");
    }

    if let Some(dep) = &info.deprecation {
        println!("Status:      DEPRECATED");
        if let Some(reason) = &dep.reason {
            println!("  Reason:    {reason}");
        }
        if let Some(repl) = &dep.replacement {
            println!("  Replace:   {repl}");
        }
    }
    if let Some(v) = &info.available_version {
        println!("Available:   v{v} (update available)");
    }

    if !info.skill_files.is_empty() {
        println!();
        println!("Files:");
        for entry in &info.skill_files {
            let indent = "  ".repeat(entry.indent_level + 1);
            if entry.is_dir {
                println!("{indent}{}/", entry.name);
            } else {
                println!("{indent}{}", entry.name);
            }
        }
    }
}

#[cfg(feature = "llm")]
pub fn print_diff_output(output: &DiffOutput) {
    if output.is_up_to_date {
        println!("{} is up to date with source.", output.artifact_name);
        return;
    }

    let installed_ver = output.installed_version.as_deref().unwrap_or("unversioned");
    let source_ver = output.source_version.as_deref().unwrap_or("unversioned");

    println!("Comparing {} ({})", output.artifact_name, output.kind);
    println!("  Installed: {installed_ver}");
    println!("  Source ({}): {source_ver}", output.source_name);
    println!();

    if let Some(analysis) = &output.analysis {
        println!("Analyzing differences...");
        println!();
        println!("{analysis}");
    } else if let Some(diff) = &output.diff_text {
        println!("Differences:");
        println!("{diff}");
    }
}

pub fn print_install_result(result: &InstallResult) {
    let version_info = result.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
    println!(
        "Installed {}{version_info} ({}) from '{}' -> {}",
        result.artifact_name,
        result.kind,
        result.source_name,
        result.dest_dir.display()
    );
}

pub fn print_install_all_result(result: &InstallAllResult) {
    if result.installed.is_empty() {
        println!("All available {}s are already installed and up to date.", result.kind);
    } else {
        for r in &result.installed {
            print_install_result(r);
        }
    }
}

pub fn print_update_all_result(result: &UpdateAllResult) {
    if result.updated.is_empty() {
        println!("All tracked {}s are up to date.", result.kind);
    } else {
        for r in &result.updated {
            print_install_result(r);
        }
    }
}

pub fn print_source_clone_start(url: &str, clone_dir: &Path) {
    println!("Cloning {url} to {}...", clone_dir.display());
}

pub fn print_source_add_result(result: &SourceAddResult) {
    println!(
        "Source '{}' registered: {} agent(s), {} skill(s) found.",
        result.name, result.agents_found, result.skills_found
    );
    for warning in &result.warnings {
        eprintln!("Warning: {}", warning.message);
    }
}

pub fn print_source_update_output(output: &SourceUpdateOutput) {
    match output {
        SourceUpdateOutput::NoGitSources => {
            println!("No git-backed sources to update.");
        }
        SourceUpdateOutput::SingleUpdate(result) => {
            println!(
                "Source '{}': {} agent(s), {} skill(s).",
                result.name, result.agents_found, result.skills_found
            );
        }
        SourceUpdateOutput::BatchUpdate(results) => {
            for result in results {
                println!(
                    "Source '{}': {} agent(s), {} skill(s).",
                    result.name, result.agents_found, result.skills_found
                );
            }
        }
    }
}

pub fn print_source_remove_result(result: &SourceRemoveResult) {
    if result.clone_deleted {
        println!("Source '{}' removed (cloned repo deleted).", result.name);
    } else {
        println!("Source '{}' removed.", result.name);
    }
}

pub fn print_config_show(result: &ConfigShowResult) {
    println!("LLM gateway: {}", result.gateway);
    println!("LLM model:   {}", result.model);
}

pub fn print_config_set(label: &str, result: &ConfigSetResult) {
    println!("LLM {label} set to: {}", result.value);
}
