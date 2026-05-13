use std::fmt::Write as FmtWrite;
use std::path::Path;

use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
use crate::info::ArtifactInfo;
use crate::install::{BatchInstallResult, InstallResult};
use crate::list::{ListKindOutput, ListOutput, Row};
use crate::outdated::OutdatedRow;
use crate::search::SearchOutput;
use crate::source::{SourceBrowseResult, SourceListResult, SourceRemoveResult, SourceScanResult};
use crate::source_update::SourceUpdateOutput;
use crate::table::Table;
use crate::types::{InstallScope, format_version_prefix};
use crate::uninstall::UninstallResult;

pub fn format_list_kind_output(output: &ListKindOutput) -> String {
    let kind = output.kind;
    let empty = vec![];
    let global = output.rows.get(&InstallScope::Global).unwrap_or(&empty);
    let local = output.rows.get(&InstallScope::Local).unwrap_or(&empty);

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
    let empty = vec![];
    let global_agents = output.agents.get(&InstallScope::Global).unwrap_or(&empty);
    let local_agents = output.agents.get(&InstallScope::Local).unwrap_or(&empty);
    let global_skills = output.skills.get(&InstallScope::Global).unwrap_or(&empty);
    let local_skills = output.skills.get(&InstallScope::Local).unwrap_or(&empty);

    if global_agents.is_empty()
        && local_agents.is_empty()
        && global_skills.is_empty()
        && local_skills.is_empty()
    {
        return "Nothing installed.\n".to_string();
    }

    let mut out = String::new();
    out.push_str(&format_section("Global agents", global_agents));
    out.push_str(&format_section("Local agents", local_agents));
    out.push_str(&format_section("Global skills", global_skills));
    out.push_str(&format_section("Local skills", local_skills));
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
            let v = format_version_prefix(a.version.as_deref());
            let _ = writeln!(out, "  {}{v}{}", a.name, a.deprecation_display);
        }
    }

    if !result.skills.is_empty() {
        if !result.agents.is_empty() {
            out.push('\n');
        }
        out.push_str("Skills:\n");
        for s in &result.skills {
            let v = format_version_prefix(s.version.as_deref());
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
    let version_info = format_version_prefix(result.version.as_deref());
    format!(
        "Installed {}{version_info} ({}) from '{}' -> {}\n",
        result.artifact_name,
        result.kind,
        result.source_name,
        result.dest_dir.display()
    )
}

pub fn format_install_all_result(result: &BatchInstallResult) -> String {
    if result.items.is_empty() {
        format!("All available {}s are already installed and up to date.\n", result.kind)
    } else {
        result.items.iter().map(format_install_result).collect()
    }
}

pub fn format_update_all_result(result: &BatchInstallResult) -> String {
    if result.items.is_empty() {
        format!("All tracked {}s are up to date.\n", result.kind)
    } else {
        result.items.iter().map(format_install_result).collect()
    }
}

pub fn format_source_clone_start(url: &str, clone_dir: &Path) -> String {
    format!("Cloning {url} to {}...\n", clone_dir.display())
}

pub fn format_source_add_result(result: &SourceScanResult) -> String {
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
    use crate::info::ArtifactInfo;
    use crate::install::{BatchInstallResult, InstallResult};
    use crate::list::{ListKindOutput, ListOutput, Row};
    use crate::outdated::OutdatedRow;
    use crate::scan::ScanWarning;
    use crate::search::{SearchOutput, SearchResult};
    use crate::source::{
        BrowseArtifact, BrowseSkill, SourceBrowseResult, SourceListEntry, SourceListResult,
        SourceRemoveResult, SourceScanResult,
    };
    use crate::source_update::SourceUpdateOutput;
    use crate::types::{ArtifactKind, Deprecation, InstallScope};
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
            rows: std::collections::BTreeMap::new(),
        };
        assert_eq!(format_list_kind_output(&output), "No agents installed.\n");
    }

    #[test]
    fn format_list_kind_output_global_only() {
        let mut rows = std::collections::BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("my-agent", "1.0.0", "src", "1.0.0")]);
        let output = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows,
        };
        let result = format_list_kind_output(&output);
        assert!(result.contains("Global agents:"), "missing section header");
        assert!(result.contains("my-agent"), "missing row data");
        assert!(!result.contains("Local agents:"), "unexpected local section");
    }

    #[test]
    fn format_list_kind_output_both_sections() {
        let mut rows = std::collections::BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("skill-a", "1.0", "src", "1.0")]);
        rows.insert(InstallScope::Local, vec![make_row("skill-b", "2.0", "src", "2.0")]);
        let output = ListKindOutput {
            kind: ArtifactKind::Skill,
            rows,
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
            agents: std::collections::BTreeMap::new(),
            skills: std::collections::BTreeMap::new(),
        };
        assert_eq!(format_list_all_output(&output), "Nothing installed.\n");
    }

    #[test]
    fn format_list_all_output_with_data() {
        let mut agents = std::collections::BTreeMap::new();
        agents.insert(InstallScope::Global, vec![make_row("agent-x", "1.0", "src", "1.0")]);
        let output = ListOutput {
            agents,
            skills: std::collections::BTreeMap::new(),
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
        let result = SourceScanResult {
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
        let result = SourceScanResult {
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

    // --- format_source_list with entries ---

    #[test]
    fn format_source_list_with_entries() {
        let result_data = SourceListResult {
            entries: vec![SourceListEntry {
                name: "guidelines".to_string(),
                kind: "local",
                location: "/home/user/repos/guidelines".to_string(),
            }],
        };
        let out = format_source_list(&result_data);
        assert!(out.contains("guidelines"));
        assert!(out.contains("local"));
        assert!(out.contains("/home/user/repos/guidelines"));
    }

    // --- format_browse_result ---

    #[test]
    fn format_browse_result_empty() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![],
            skills: vec![],
        };
        let out = format_browse_result(&result);
        assert!(out.contains("No agents or skills found in 'my-source'"));
    }

    #[test]
    fn format_browse_result_agents_only() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![BrowseArtifact {
                name: "rust-craftsperson".to_string(),
                version: Some("1.0.0".to_string()),
                deprecation_display: String::new(),
            }],
            skills: vec![],
        };
        let out = format_browse_result(&result);
        assert!(out.contains("Agents:"));
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("v1.0.0"));
        assert!(!out.contains("Skills:"));
    }

    #[test]
    fn format_browse_result_skills_only() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![],
            skills: vec![BrowseSkill {
                name: "my-skill".to_string(),
                version: None,
                deprecation_display: String::new(),
                files: vec!["tool.md".to_string()],
            }],
        };
        let out = format_browse_result(&result);
        assert!(!out.contains("Agents:"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("tool.md"));
    }

    #[test]
    fn format_browse_result_agents_and_skills() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![BrowseArtifact {
                name: "my-agent".to_string(),
                version: None,
                deprecation_display: String::new(),
            }],
            skills: vec![BrowseSkill {
                name: "my-skill".to_string(),
                version: Some("2.0.0".to_string()),
                deprecation_display: String::new(),
                files: vec![],
            }],
        };
        let out = format_browse_result(&result);
        assert!(out.contains("Agents:"));
        assert!(out.contains("my-agent"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("v2.0.0"));
    }

    // --- format_info ---

    fn minimal_info(name: &str, kind: ArtifactKind) -> ArtifactInfo {
        ArtifactInfo {
            name: name.to_string(),
            kind,
            scope: "global",
            path: PathBuf::from(format!("{name}.md")),
            version: None,
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: false,
            deprecation: None,
            available_version: None,
            skill_files: vec![],
        }
    }

    #[test]
    fn format_info_basic_fields() {
        let info = minimal_info("my-agent", ArtifactKind::Agent);
        let out = format_info(&info);
        assert!(out.contains("Name:        my-agent"));
        assert!(out.contains("Type:        agent"));
        assert!(out.contains("Scope:       global"));
    }

    #[test]
    fn format_info_with_version_and_source() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.version = Some("1.2.3".to_string());
        info.installed_at = Some("2024-01-01T00:00:00Z".to_string());
        info.source_display = Some("guidelines".to_string());
        let out = format_info(&info);
        assert!(out.contains("Version:     1.2.3"));
        assert!(out.contains("Installed:   2024-01-01T00:00:00Z"));
        assert!(out.contains("Source:      guidelines"));
    }

    #[test]
    fn format_info_locally_modified() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.locally_modified = true;
        info.disk_checksum = Some("sha256:abcdef".to_string());
        let out = format_info(&info);
        assert!(out.contains("locally modified"));
        assert!(out.contains("sha256:abcdef"));
    }

    #[test]
    fn format_info_untracked() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.untracked = true;
        let out = format_info(&info);
        assert!(out.contains("untracked"));
    }

    #[test]
    fn format_info_deprecated_with_reason_and_replacement() {
        let mut info = minimal_info("old-agent", ArtifactKind::Agent);
        info.deprecation = Some(Deprecation {
            reason: Some("Too old".to_string()),
            replacement: Some("new-agent".to_string()),
        });
        let out = format_info(&info);
        assert!(out.contains("DEPRECATED"));
        assert!(out.contains("Too old"));
        assert!(out.contains("new-agent"));
    }

    #[test]
    fn format_info_with_available_version() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.available_version = Some("2.0.0".to_string());
        let out = format_info(&info);
        assert!(out.contains("v2.0.0"));
        assert!(out.contains("update available"));
    }

    #[test]
    fn format_info_with_skill_files() {
        use crate::info::SkillFileEntry;
        let mut info = minimal_info("my-skill", ArtifactKind::Skill);
        info.skill_files = vec![
            SkillFileEntry {
                name: "SKILL.md".to_string(),
                is_dir: false,
                indent_level: 0,
            },
            SkillFileEntry {
                name: "tools".to_string(),
                is_dir: true,
                indent_level: 0,
            },
            SkillFileEntry {
                name: "helper.py".to_string(),
                is_dir: false,
                indent_level: 1,
            },
        ];
        let out = format_info(&info);
        assert!(out.contains("Files:"));
        assert!(out.contains("SKILL.md"));
        assert!(out.contains("tools/"));
        assert!(out.contains("helper.py"));
    }

    // --- format_install_all_result ---

    #[test]
    fn format_install_all_result_empty() {
        let result = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
        };
        let out = format_install_all_result(&result);
        assert!(out.contains("already installed and up to date"));
        assert!(out.contains("agent"));
    }

    #[test]
    fn format_install_all_result_with_items() {
        let result = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-agent".to_string(),
                kind: ArtifactKind::Agent,
                source_name: "guidelines".to_string(),
                dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
                version: Some("1.0.0".to_string()),
            }],
            kind: ArtifactKind::Agent,
        };
        let out = format_install_all_result(&result);
        assert!(out.contains("my-agent"));
        assert!(out.contains("guidelines"));
    }

    // --- format_update_all_result ---

    #[test]
    fn format_update_all_result_empty() {
        let result = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Skill,
        };
        let out = format_update_all_result(&result);
        assert!(out.contains("up to date"));
        assert!(out.contains("skill"));
    }

    #[test]
    fn format_update_all_result_with_items() {
        let result = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-skill".to_string(),
                kind: ArtifactKind::Skill,
                source_name: "guidelines".to_string(),
                dest_dir: PathBuf::from("/home/user/.config/cmx/skills"),
                version: None,
            }],
            kind: ArtifactKind::Skill,
        };
        let out = format_update_all_result(&result);
        assert!(out.contains("my-skill"));
    }

    // --- format_source_update_output ---

    #[test]
    fn format_source_update_output_no_git_sources() {
        let out = format_source_update_output(&SourceUpdateOutput::NoGitSources);
        assert_eq!(out, "No git-backed sources to update.\n");
    }

    #[test]
    fn format_source_update_output_single_update() {
        let out =
            format_source_update_output(&SourceUpdateOutput::SingleUpdate(SourceScanResult {
                name: "guidelines".to_string(),
                agents_found: 5,
                skills_found: 3,
                warnings: vec![],
            }));
        assert!(out.contains("guidelines"));
        assert!(out.contains("5 agent(s)"));
        assert!(out.contains("3 skill(s)"));
    }

    #[test]
    fn format_source_update_output_batch_update() {
        let out = format_source_update_output(&SourceUpdateOutput::BatchUpdate(vec![
            SourceScanResult {
                name: "source-a".to_string(),
                agents_found: 1,
                skills_found: 0,
                warnings: vec![],
            },
            SourceScanResult {
                name: "source-b".to_string(),
                agents_found: 2,
                skills_found: 4,
                warnings: vec![],
            },
        ]));
        assert!(out.contains("source-a"));
        assert!(out.contains("source-b"));
        assert!(out.contains("2 agent(s)"));
        assert!(out.contains("4 skill(s)"));
    }

    // --- format_source_remove_result ---

    #[test]
    fn format_source_remove_result_with_clone_deleted() {
        let result = SourceRemoveResult {
            name: "git-source".to_string(),
            clone_deleted: true,
        };
        let out = format_source_remove_result(&result);
        assert!(out.contains("git-source"));
        assert!(out.contains("cloned repo deleted"));
    }

    #[test]
    fn format_source_remove_result_without_clone() {
        let result = SourceRemoveResult {
            name: "local-source".to_string(),
            clone_deleted: false,
        };
        let out = format_source_remove_result(&result);
        assert!(out.contains("local-source"));
        assert!(!out.contains("cloned repo deleted"));
    }
}
