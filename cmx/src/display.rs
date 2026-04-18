use std::fmt::Write as FmtWrite;
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

pub fn format_list_kind_output(output: &ListKindOutput) -> String {
    let kind = output.kind;
    let global = &output.global_rows;
    let local = &output.local_rows;

    if global.is_empty() && local.is_empty() {
        return format!("No {kind}s installed.\n");
    }

    let mut out = String::new();

    if !global.is_empty() {
        let _ = writeln!(out, "Global {kind}s:");
        out.push_str(&format_table(global));
    }

    if !local.is_empty() {
        if !global.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(out, "Local {kind}s:");
        out.push_str(&format_table(local));
    }

    out
}

pub fn format_list_all_output(output: &ListOutput) -> String {
    if output.global_agents.is_empty()
        && output.local_agents.is_empty()
        && output.global_skills.is_empty()
        && output.local_skills.is_empty()
    {
        return "Nothing installed.\n".to_string();
    }

    let mut out = String::new();
    out.push_str(&format_section("Global agents", &output.global_agents));
    out.push_str(&format_section("Local agents", &output.local_agents));
    out.push_str(&format_section("Global skills", &output.global_skills));
    out.push_str(&format_section("Local skills", &output.local_skills));
    out
}

pub fn format_table(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::new();
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
    .render()
}

pub fn format_section(label: &str, rows: &[Row]) -> String {
    let mut out = format!("{label}:\n");
    if rows.is_empty() {
        out.push_str("  (none)\n");
    } else {
        out.push_str(&format_table(rows));
    }
    out.push('\n');
    out
}

pub fn format_source_list(result: &SourceListResult) -> String {
    if result.entries.is_empty() {
        return "No sources registered.\n\nAdd one with: cmx source add <name> <path-or-url>\n"
            .to_string();
    }

    let mut out = String::new();
    for entry in &result.entries {
        let _ = writeln!(out, "  {:<28} ({}) {}", entry.name, entry.kind, entry.location);
    }
    out
}

pub fn format_browse_result(result: &SourceBrowseResult) -> String {
    let name = &result.source_name;

    if result.agents.is_empty() && result.skills.is_empty() {
        return format!("No agents or skills found in '{name}'.\n");
    }

    let mut out = String::new();

    if !result.agents.is_empty() {
        out.push_str("Agents:\n");
        for a in &result.agents {
            let v = a.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            let _ = writeln!(out, "  {}{v}{}", a.name, a.deprecation_display);
        }
    }

    if !result.skills.is_empty() {
        if !result.agents.is_empty() {
            out.push('\n');
        }
        out.push_str("Skills:\n");
        for s in &result.skills {
            let v = s.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            let _ = writeln!(out, "  {}{v}{}", s.name, s.deprecation_display);
            for f in &s.files {
                let _ = writeln!(out, "    {f}");
            }
        }
    }

    out
}

pub fn format_outdated(rows: &[OutdatedRow]) -> String {
    if rows.is_empty() {
        return "Everything is up to date.\n".to_string();
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
    .render()
}

pub fn format_uninstall_result(result: &UninstallResult) -> String {
    let mut out =
        format!("Uninstalled {} ({}) from {} scope.\n", result.name, result.kind, result.scope);
    if !result.was_tracked {
        out.push_str("  (no lock file entry found — artifact was untracked)\n");
    }
    out
}

pub fn format_search_results(output: &SearchOutput) -> String {
    let query = &output.query;
    let results = &output.results;

    if results.is_empty() {
        return format!("No results for '{query}'.\n");
    }

    let mut out = Table {
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
    .render();

    out.push('\n');
    let _ = writeln!(out, "{} result(s) found.", results.len());
    out
}

pub fn format_info(info: &ArtifactInfo) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Name:        {}", info.name);
    let _ = writeln!(out, "Type:        {}", info.kind);
    let _ = writeln!(out, "Scope:       {}", info.scope);
    let _ = writeln!(out, "Path:        {}", info.path.display());

    if let Some(v) = &info.version {
        let _ = writeln!(out, "Version:     {v}");
    }
    if let Some(at) = &info.installed_at {
        let _ = writeln!(out, "Installed:   {at}");
    }
    if let Some(src) = &info.source_display {
        let _ = writeln!(out, "Source:      {src}");
    }
    if let Some(cs) = &info.source_checksum {
        let _ = writeln!(out, "Source SHA:  {cs}");
    }
    if let Some(cs) = &info.installed_checksum {
        let _ = writeln!(out, "Install SHA: {cs}");
    }
    if info.locally_modified {
        let disk_cs = info.disk_checksum.as_deref().unwrap_or("unknown");
        let _ = writeln!(out, "Disk SHA:    {disk_cs}  (locally modified)");
    }
    if info.untracked {
        out.push_str("Lock entry:  (none — untracked)\n");
    }

    if let Some(dep) = &info.deprecation {
        out.push_str("Status:      DEPRECATED\n");
        if let Some(reason) = &dep.reason {
            let _ = writeln!(out, "  Reason:    {reason}");
        }
        if let Some(repl) = &dep.replacement {
            let _ = writeln!(out, "  Replace:   {repl}");
        }
    }
    if let Some(v) = &info.available_version {
        let _ = writeln!(out, "Available:   v{v} (update available)");
    }

    if !info.skill_files.is_empty() {
        out.push('\n');
        out.push_str("Files:\n");
        for entry in &info.skill_files {
            let indent = "  ".repeat(entry.indent_level + 1);
            if entry.is_dir {
                let _ = writeln!(out, "{indent}{}/", entry.name);
            } else {
                let _ = writeln!(out, "{indent}{}", entry.name);
            }
        }
    }

    out
}

#[cfg(feature = "llm")]
pub fn format_diff_output(output: &DiffOutput) -> String {
    if output.is_up_to_date {
        return format!("{} is up to date with source.\n", output.artifact_name);
    }

    let installed_ver = output.installed_version.as_deref().unwrap_or("unversioned");
    let source_ver = output.source_version.as_deref().unwrap_or("unversioned");

    let mut out = format!("Comparing {} ({})\n", output.artifact_name, output.kind);
    let _ = writeln!(out, "  Installed: {installed_ver}");
    let _ = writeln!(out, "  Source ({}): {source_ver}", output.source_name);
    out.push('\n');

    if let Some(analysis) = &output.analysis {
        out.push_str("Analyzing differences...\n");
        out.push('\n');
        let _ = writeln!(out, "{analysis}");
    } else if let Some(diff) = &output.diff_text {
        out.push_str("Differences:\n");
        let _ = writeln!(out, "{diff}");
    }

    out
}

pub fn format_install_result(result: &InstallResult) -> String {
    let version_info = result.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
    format!(
        "Installed {}{version_info} ({}) from '{}' -> {}\n",
        result.artifact_name,
        result.kind,
        result.source_name,
        result.dest_dir.display()
    )
}

pub fn format_install_all_result(result: &InstallAllResult) -> String {
    if result.installed.is_empty() {
        format!("All available {}s are already installed and up to date.\n", result.kind)
    } else {
        result.installed.iter().map(format_install_result).collect()
    }
}

pub fn format_update_all_result(result: &UpdateAllResult) -> String {
    if result.updated.is_empty() {
        format!("All tracked {}s are up to date.\n", result.kind)
    } else {
        result.updated.iter().map(format_install_result).collect()
    }
}

pub fn format_source_clone_start(url: &str, clone_dir: &Path) -> String {
    format!("Cloning {url} to {}...\n", clone_dir.display())
}

pub fn format_source_add_result(result: &SourceAddResult) -> String {
    let mut out = format!(
        "Source '{}' registered: {} agent(s), {} skill(s) found.\n",
        result.name, result.agents_found, result.skills_found
    );
    for warning in &result.warnings {
        let _ = writeln!(out, "Warning: {}", warning.message);
    }
    out
}

pub fn format_source_update_output(output: &SourceUpdateOutput) -> String {
    match output {
        SourceUpdateOutput::NoGitSources => "No git-backed sources to update.\n".to_string(),
        SourceUpdateOutput::SingleUpdate(result) => format!(
            "Source '{}': {} agent(s), {} skill(s).\n",
            result.name, result.agents_found, result.skills_found
        ),
        SourceUpdateOutput::BatchUpdate(results) => {
            let mut out = String::new();
            for result in results {
                let _ = writeln!(
                    out,
                    "Source '{}': {} agent(s), {} skill(s).",
                    result.name, result.agents_found, result.skills_found
                );
            }
            out
        }
    }
}

pub fn format_source_remove_result(result: &SourceRemoveResult) -> String {
    if result.clone_deleted {
        format!("Source '{}' removed (cloned repo deleted).\n", result.name)
    } else {
        format!("Source '{}' removed.\n", result.name)
    }
}

pub fn format_config_show(result: &ConfigShowResult) -> String {
    format!("LLM gateway: {}\nLLM model:   {}\n", result.gateway, result.model)
}

pub fn format_config_set(label: &str, result: &ConfigSetResult) -> String {
    format!("LLM {label} set to: {}\n", result.value)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::install::InstallResult;
    use crate::list::{ListKindOutput, ListOutput, Row};
    use crate::outdated::OutdatedRow;
    use crate::scan::ScanWarning;
    use crate::search::{SearchOutput, SearchResult};
    use crate::source::{SourceAddResult, SourceListResult};
    use crate::types::ArtifactKind;
    use crate::uninstall::UninstallResult;

    fn make_row(name: &str, installed: &str, source: &str, available: &str) -> Row {
        Row {
            name: name.to_string(),
            installed: installed.to_string(),
            source: source.to_string(),
            available: available.to_string(),
            status: "✅",
        }
    }

    // --- format_list_kind_output ---

    #[test]
    fn format_list_kind_output_empty() {
        let output = ListKindOutput {
            kind: ArtifactKind::Agent,
            global_rows: vec![],
            local_rows: vec![],
        };
        assert_eq!(format_list_kind_output(&output), "No agents installed.\n");
    }

    #[test]
    fn format_list_kind_output_global_only() {
        let output = ListKindOutput {
            kind: ArtifactKind::Agent,
            global_rows: vec![make_row("my-agent", "1.0.0", "src", "1.0.0")],
            local_rows: vec![],
        };
        let result = format_list_kind_output(&output);
        assert!(result.contains("Global agents:"), "missing section header");
        assert!(result.contains("my-agent"), "missing row data");
        assert!(!result.contains("Local agents:"), "unexpected local section");
    }

    #[test]
    fn format_list_kind_output_both_sections() {
        let output = ListKindOutput {
            kind: ArtifactKind::Skill,
            global_rows: vec![make_row("skill-a", "1.0", "src", "1.0")],
            local_rows: vec![make_row("skill-b", "2.0", "src", "2.0")],
        };
        let result = format_list_kind_output(&output);
        assert!(result.contains("Global skills:"));
        assert!(result.contains("Local skills:"));
        assert!(result.contains("skill-a"));
        assert!(result.contains("skill-b"));
    }

    // --- format_list_all_output ---

    #[test]
    fn format_list_all_output_empty() {
        let output = ListOutput {
            global_agents: vec![],
            local_agents: vec![],
            global_skills: vec![],
            local_skills: vec![],
        };
        assert_eq!(format_list_all_output(&output), "Nothing installed.\n");
    }

    #[test]
    fn format_list_all_output_with_data() {
        let output = ListOutput {
            global_agents: vec![make_row("agent-x", "1.0", "src", "1.0")],
            local_agents: vec![],
            global_skills: vec![],
            local_skills: vec![],
        };
        let result = format_list_all_output(&output);
        assert!(result.contains("Global agents:"));
        assert!(result.contains("agent-x"));
    }

    // --- format_section ---

    #[test]
    fn format_section_empty_rows_shows_none() {
        let result = format_section("My Section", &[]);
        assert_eq!(result, "My Section:\n  (none)\n\n");
    }

    #[test]
    fn format_section_with_rows() {
        let rows = vec![make_row("item", "1.0", "src", "1.0")];
        let result = format_section("My Section", &rows);
        assert!(result.starts_with("My Section:\n"));
        assert!(result.contains("item"));
        assert!(result.ends_with('\n'));
    }

    // --- format_source_list ---

    #[test]
    fn format_source_list_empty() {
        let result_data = SourceListResult { entries: vec![] };
        let out = format_source_list(&result_data);
        assert!(out.contains("No sources registered."));
        assert!(out.contains("cmx source add"));
    }

    // --- format_outdated ---

    #[test]
    fn format_outdated_empty() {
        assert_eq!(format_outdated(&[]), "Everything is up to date.\n");
    }

    #[test]
    fn format_outdated_with_rows() {
        let rows = vec![OutdatedRow {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            installed_version: "1.0.0".to_string(),
            available_version: "2.0.0".to_string(),
            source: "guidelines".to_string(),
            status: "update".to_string(),
        }];
        let out = format_outdated(&rows);
        assert!(out.contains("my-agent"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("2.0.0"));
    }

    // --- format_uninstall_result ---

    #[test]
    fn format_uninstall_result_tracked() {
        let result = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: true,
        };
        let out = format_uninstall_result(&result);
        assert!(out.contains("Uninstalled my-agent"));
        assert!(!out.contains("untracked"));
    }

    #[test]
    fn format_uninstall_result_untracked() {
        let result = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: false,
        };
        let out = format_uninstall_result(&result);
        assert!(out.contains("untracked"));
    }

    // --- format_search_results ---

    #[test]
    fn format_search_results_empty() {
        let output = SearchOutput {
            query: "foo".to_string(),
            results: vec![],
        };
        assert_eq!(format_search_results(&output), "No results for 'foo'.\n");
    }

    #[test]
    fn format_search_results_with_data() {
        let output = SearchOutput {
            query: "rust".to_string(),
            results: vec![SearchResult {
                name: "rust-craftsperson".to_string(),
                kind: "agent".to_string(),
                version: "1.0.0".to_string(),
                source: "guidelines".to_string(),
                description: "Rust expert".to_string(),
            }],
        };
        let out = format_search_results(&output);
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("1 result(s) found."));
    }

    // --- format_install_result ---

    #[test]
    fn format_install_result_with_version() {
        let result = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
            version: Some("1.0.0".to_string()),
        };
        let out = format_install_result(&result);
        assert!(out.contains("my-agent"));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("guidelines"));
    }

    #[test]
    fn format_install_result_without_version() {
        let result = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
            version: None,
        };
        let out = format_install_result(&result);
        assert!(!out.contains(" v"));
    }

    // --- format_source_clone_start ---

    #[test]
    fn format_source_clone_start_output() {
        let out = format_source_clone_start(
            "https://github.com/org/repo",
            Path::new("/home/user/.config/cmx/clones/repo"),
        );
        assert!(out.contains("Cloning https://github.com/org/repo"));
        assert!(out.contains("clones/repo"));
    }

    // --- format_source_add_result ---

    #[test]
    fn format_source_add_result_no_warnings() {
        let result = SourceAddResult {
            name: "my-source".to_string(),
            agents_found: 3,
            skills_found: 1,
            warnings: vec![],
        };
        let out = format_source_add_result(&result);
        assert!(out.contains("my-source"));
        assert!(out.contains("3 agent(s)"));
        assert!(!out.contains("Warning:"));
    }

    #[test]
    fn format_source_add_result_with_warnings() {
        let result = SourceAddResult {
            name: "my-source".to_string(),
            agents_found: 0,
            skills_found: 0,
            warnings: vec![ScanWarning {
                message: "something fishy".to_string(),
            }],
        };
        let out = format_source_add_result(&result);
        assert!(out.contains("Warning: something fishy"));
    }

    // --- format_config_show ---

    #[test]
    fn format_config_show_output() {
        let result = ConfigShowResult {
            gateway: "ollama".to_string(),
            model: "llama3".to_string(),
        };
        let out = format_config_show(&result);
        assert!(out.contains("LLM gateway: ollama"));
        assert!(out.contains("LLM model:   llama3"));
    }

    // --- format_config_set ---

    #[test]
    fn format_config_set_output() {
        let result = ConfigSetResult {
            field: "model",
            value: "gemma2".to_string(),
        };
        let out = format_config_set("model", &result);
        assert_eq!(out, "LLM model set to: gemma2\n");
    }
}
