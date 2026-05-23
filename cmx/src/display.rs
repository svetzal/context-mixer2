use std::fmt;

use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
use crate::info::ArtifactInfo;
use crate::install::{BatchInstallResult, InstallResult};
use crate::list::{ListKindOutput, ListOutput, section_str, table_str};
use crate::outdated::OutdatedReport;
use crate::search::SearchOutput;
use crate::source::{SourceBrowseResult, SourceListResult, SourceRemoveResult, SourceScanResult};
use crate::source_update::SourceUpdateOutput;
use crate::table::Table;
use crate::types::{InstallScope, format_version_prefix};
use crate::uninstall::UninstallResult;

impl fmt::Display for SourceListResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entries.is_empty() {
            return write!(
                f,
                "No sources registered.\n\nAdd one with: cmx source add <name> <path-or-url>\n"
            );
        }
        for entry in &self.entries {
            writeln!(f, "  {:<28} ({}) {}", entry.name, entry.kind, entry.location)?;
        }
        Ok(())
    }
}

impl fmt::Display for SourceBrowseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.source_name;
        if self.agents.is_empty() && self.skills.is_empty() {
            return writeln!(f, "No agents or skills found in '{name}'.");
        }
        if !self.agents.is_empty() {
            writeln!(f, "Agents:")?;
            for a in &self.agents {
                let v = format_version_prefix(a.version.as_deref());
                writeln!(f, "  {}{v}{}", a.name, a.deprecation_display)?;
            }
        }
        if !self.skills.is_empty() {
            if !self.agents.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Skills:")?;
            for s in &self.skills {
                let v = format_version_prefix(s.version.as_deref());
                writeln!(f, "  {}{v}{}", s.name, s.deprecation_display)?;
                for file in &s.files {
                    writeln!(f, "    {file}")?;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for SourceScanResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Source '{}' registered: {} agent(s), {} skill(s) found.",
            self.name, self.agents_found, self.skills_found
        )?;
        for warning in &self.warnings {
            writeln!(f, "Warning: {}", warning.message)?;
        }
        Ok(())
    }
}

impl fmt::Display for SourceRemoveResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.clone_deleted {
            writeln!(f, "Source '{}' removed (cloned repo deleted).", self.name)
        } else {
            writeln!(f, "Source '{}' removed.", self.name)
        }
    }
}

impl fmt::Display for InstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let version_info = format_version_prefix(self.version.as_deref());
        writeln!(
            f,
            "Installed {}{version_info} ({}) from '{}' -> {}",
            self.artifact_name,
            self.kind,
            self.source_name,
            self.dest_dir.display()
        )
    }
}

impl fmt::Display for BatchInstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.items.is_empty() {
            if self.is_update {
                writeln!(f, "All tracked {}s are up to date.", self.kind)
            } else {
                writeln!(f, "All available {}s are already installed and up to date.", self.kind)
            }
        } else {
            for item in &self.items {
                write!(f, "{item}")?;
            }
            Ok(())
        }
    }
}

impl fmt::Display for ListKindOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.kind;
        let empty = vec![];
        let global = self.rows.get(&InstallScope::Global).unwrap_or(&empty);
        let local = self.rows.get(&InstallScope::Local).unwrap_or(&empty);

        if global.is_empty() && local.is_empty() {
            return writeln!(f, "No {kind}s installed.");
        }

        if !global.is_empty() {
            writeln!(f, "Global {kind}s:")?;
            write!(f, "{}", table_str(global))?;
        }

        if !local.is_empty() {
            if !global.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Local {kind}s:")?;
            write!(f, "{}", table_str(local))?;
        }

        Ok(())
    }
}

impl fmt::Display for ListOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let empty = vec![];
        let global_agents = self.agents.get(&InstallScope::Global).unwrap_or(&empty);
        let local_agents = self.agents.get(&InstallScope::Local).unwrap_or(&empty);
        let global_skills = self.skills.get(&InstallScope::Global).unwrap_or(&empty);
        let local_skills = self.skills.get(&InstallScope::Local).unwrap_or(&empty);

        if global_agents.is_empty()
            && local_agents.is_empty()
            && global_skills.is_empty()
            && local_skills.is_empty()
        {
            return writeln!(f, "Nothing installed.");
        }

        write!(f, "{}", section_str("Global agents", global_agents))?;
        write!(f, "{}", section_str("Local agents", local_agents))?;
        write!(f, "{}", section_str("Global skills", global_skills))?;
        write!(f, "{}", section_str("Local skills", local_skills))?;
        Ok(())
    }
}

impl fmt::Display for OutdatedReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = &self.0;
        if rows.is_empty() {
            return writeln!(f, "Everything is up to date.");
        }
        write!(
            f,
            "{}",
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
        )
    }
}

impl fmt::Display for SearchOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let query = &self.query;
        let results = &self.results;

        if results.is_empty() {
            return writeln!(f, "No results for '{query}'.");
        }

        let table = Table {
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

        write!(f, "{table}\n{} result(s) found.\n", results.len())
    }
}

impl fmt::Display for ArtifactInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Name:        {}", self.name)?;
        writeln!(f, "Type:        {}", self.kind)?;
        writeln!(f, "Scope:       {}", self.scope)?;
        writeln!(f, "Path:        {}", self.path.display())?;

        if let Some(v) = &self.version {
            writeln!(f, "Version:     {v}")?;
        }
        if let Some(at) = &self.installed_at {
            writeln!(f, "Installed:   {at}")?;
        }
        if let Some(src) = &self.source_display {
            writeln!(f, "Source:      {src}")?;
        }
        if let Some(cs) = &self.source_checksum {
            writeln!(f, "Source SHA:  {cs}")?;
        }
        if let Some(cs) = &self.installed_checksum {
            writeln!(f, "Install SHA: {cs}")?;
        }
        if self.locally_modified {
            let disk_cs = self.disk_checksum.as_deref().unwrap_or("unknown");
            writeln!(f, "Disk SHA:    {disk_cs}  (locally modified)")?;
        }
        if self.untracked {
            writeln!(f, "Lock entry:  (none — untracked)")?;
        }

        if let Some(dep) = &self.deprecation {
            writeln!(f, "Status:      DEPRECATED")?;
            if let Some(reason) = &dep.reason {
                writeln!(f, "  Reason:    {reason}")?;
            }
            if let Some(repl) = &dep.replacement {
                writeln!(f, "  Replace:   {repl}")?;
            }
        }
        if let Some(v) = &self.available_version {
            writeln!(f, "Available:   v{v} (update available)")?;
        }

        if !self.skill_files.is_empty() {
            writeln!(f)?;
            writeln!(f, "Files:")?;
            for entry in &self.skill_files {
                let indent = "  ".repeat(entry.indent_level + 1);
                if entry.is_dir {
                    writeln!(f, "{indent}{}/", entry.name)?;
                } else {
                    writeln!(f, "{indent}{}", entry.name)?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(feature = "llm")]
impl fmt::Display for DiffOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_up_to_date {
            return writeln!(f, "{} is up to date with source.", self.artifact_name);
        }

        let installed_ver = self.installed_version.as_deref().unwrap_or("unversioned");
        let source_ver = self.source_version.as_deref().unwrap_or("unversioned");

        writeln!(f, "Comparing {} ({})", self.artifact_name, self.kind)?;
        writeln!(f, "  Installed: {installed_ver}")?;
        writeln!(f, "  Source ({}): {source_ver}", self.source_name)?;
        writeln!(f)?;

        if let Some(analysis) = &self.analysis {
            writeln!(f, "Analyzing differences...")?;
            writeln!(f)?;
            writeln!(f, "{analysis}")?;
        } else if let Some(diff) = &self.diff_text {
            writeln!(f, "Differences:")?;
            writeln!(f, "{diff}")?;
        }

        Ok(())
    }
}

impl fmt::Display for UninstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Uninstalled {} ({}) from {} scope.", self.name, self.kind, self.scope)?;
        if !self.was_tracked {
            writeln!(f, "  (no lock file entry found — artifact was untracked)")?;
        }
        Ok(())
    }
}

impl fmt::Display for ConfigShowResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLM gateway: {}\nLLM model:   {}\n", self.gateway, self.model)
    }
}

impl fmt::Display for ConfigSetResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "LLM {} set to: {}", self.field, self.value)
    }
}

impl fmt::Display for SourceUpdateOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceUpdateOutput::NoGitSources => writeln!(f, "No git-backed sources to update."),
            SourceUpdateOutput::SingleUpdate(result) => writeln!(
                f,
                "Source '{}': {} agent(s), {} skill(s).",
                result.name, result.agents_found, result.skills_found
            ),
            SourceUpdateOutput::BatchUpdate(results) => {
                for result in results {
                    writeln!(
                        f,
                        "Source '{}': {} agent(s), {} skill(s).",
                        result.name, result.agents_found, result.skills_found
                    )?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
    use crate::install::{BatchInstallResult, InstallResult};
    use crate::scan::ScanWarning;
    use crate::source::{
        BrowseArtifact, BrowseSkill, SourceBrowseResult, SourceListEntry, SourceListResult,
        SourceRemoveResult, SourceScanResult,
    };
    use crate::source_update::SourceUpdateOutput;
    use crate::types::ArtifactKind;
    use crate::uninstall::UninstallResult;

    // --- SourceListResult ---

    #[test]
    fn source_list_result_empty_shows_help() {
        let r = SourceListResult { entries: vec![] };
        let out = r.to_string();
        assert!(out.contains("No sources registered."));
        assert!(out.contains("cmx source add"));
    }

    #[test]
    fn source_list_result_with_entry_shows_columns() {
        let r = SourceListResult {
            entries: vec![SourceListEntry {
                name: "guidelines".to_string(),
                kind: "local",
                location: "/repos/guidelines".to_string(),
            }],
        };
        let out = r.to_string();
        assert!(out.contains("guidelines"));
        assert!(out.contains("local"));
        assert!(out.contains("/repos/guidelines"));
    }

    // --- SourceBrowseResult ---

    #[test]
    fn source_browse_result_empty_shows_message() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
            agents: vec![],
            skills: vec![],
        };
        assert!(r.to_string().contains("No agents or skills found in 'src'"));
    }

    #[test]
    fn source_browse_result_agents_only() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
            agents: vec![BrowseArtifact {
                name: "rust-craftsperson".to_string(),
                version: Some("1.0.0".to_string()),
                deprecation_display: String::new(),
            }],
            skills: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("v1.0.0"));
        assert!(!out.contains("Skills:"));
    }

    #[test]
    fn source_browse_result_skills_only() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
            agents: vec![],
            skills: vec![BrowseSkill {
                name: "my-skill".to_string(),
                version: None,
                deprecation_display: String::new(),
                files: vec!["tool.md".to_string()],
            }],
        };
        let out = r.to_string();
        assert!(!out.contains("Agents:"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("tool.md"));
    }

    #[test]
    fn source_browse_result_both_sections() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
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
        let out = r.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("v2.0.0"));
    }

    // --- SourceScanResult ---

    #[test]
    fn source_scan_result_no_warnings() {
        let r = SourceScanResult {
            name: "src".to_string(),
            agents_found: 3,
            skills_found: 2,
            warnings: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("src"));
        assert!(out.contains("3 agent(s)"));
        assert!(out.contains("2 skill(s)"));
        assert!(!out.contains("Warning:"));
    }

    #[test]
    fn source_scan_result_with_warnings() {
        let r = SourceScanResult {
            name: "src".to_string(),
            agents_found: 0,
            skills_found: 0,
            warnings: vec![ScanWarning {
                message: "something odd".to_string(),
            }],
        };
        assert!(r.to_string().contains("Warning: something odd"));
    }

    // --- SourceRemoveResult ---

    #[test]
    fn source_remove_result_with_clone() {
        let r = SourceRemoveResult {
            name: "git-src".to_string(),
            clone_deleted: true,
        };
        assert!(r.to_string().contains("cloned repo deleted"));
    }

    #[test]
    fn source_remove_result_without_clone() {
        let r = SourceRemoveResult {
            name: "local-src".to_string(),
            clone_deleted: false,
        };
        let out = r.to_string();
        assert!(out.contains("local-src"));
        assert!(!out.contains("cloned repo deleted"));
    }

    // --- InstallResult ---

    #[test]
    fn install_result_with_version() {
        let r = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "src".to_string(),
            dest_dir: PathBuf::from("/agents"),
            version: Some("1.0.0".to_string()),
        };
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("src"));
    }

    #[test]
    fn install_result_without_version() {
        let r = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "src".to_string(),
            dest_dir: PathBuf::from("/agents"),
            version: None,
        };
        assert!(!r.to_string().contains(" v"));
    }

    // --- BatchInstallResult ---

    #[test]
    fn batch_install_result_empty_install_mode() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        assert!(r.to_string().contains("already installed and up to date"));
    }

    #[test]
    fn batch_install_result_empty_update_mode() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Skill,
            is_update: true,
        };
        assert!(r.to_string().contains("up to date"));
        assert!(r.to_string().contains("skill"));
    }

    #[test]
    fn batch_install_result_with_items() {
        let r = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-agent".to_string(),
                kind: ArtifactKind::Agent,
                source_name: "src".to_string(),
                dest_dir: PathBuf::from("/agents"),
                version: None,
            }],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        assert!(r.to_string().contains("my-agent"));
    }

    // --- UninstallResult ---

    #[test]
    fn uninstall_result_tracked() {
        let r = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: true,
        };
        let out = r.to_string();
        assert!(out.contains("Uninstalled my-agent"));
        assert!(!out.contains("untracked"));
    }

    #[test]
    fn uninstall_result_untracked() {
        let r = UninstallResult {
            name: "mystery".to_string(),
            kind: ArtifactKind::Skill,
            scope: "local",
            was_tracked: false,
        };
        assert!(r.to_string().contains("untracked"));
    }

    // --- ConfigShowResult ---

    #[test]
    fn config_show_result() {
        let r = ConfigShowResult {
            gateway: "openai".to_string(),
            model: "gpt-4".to_string(),
        };
        let out = r.to_string();
        assert!(out.contains("LLM gateway: openai"));
        assert!(out.contains("LLM model:   gpt-4"));
    }

    // --- ConfigSetResult ---

    #[test]
    fn config_set_result_model() {
        let r = ConfigSetResult {
            field: "model",
            value: "llama3".to_string(),
        };
        assert_eq!(r.to_string(), "LLM model set to: llama3\n");
    }

    // --- SourceUpdateOutput ---

    #[test]
    fn source_update_output_no_git_sources() {
        assert!(SourceUpdateOutput::NoGitSources.to_string().contains("No git-backed sources"));
    }

    #[test]
    fn source_update_output_single_update() {
        let r = SourceUpdateOutput::SingleUpdate(SourceScanResult {
            name: "src".to_string(),
            agents_found: 2,
            skills_found: 1,
            warnings: vec![],
        });
        let out = r.to_string();
        assert!(out.contains("Source 'src'"));
        assert!(out.contains("2 agent(s)"));
    }

    #[test]
    fn source_update_output_batch_update() {
        let r = SourceUpdateOutput::BatchUpdate(vec![
            SourceScanResult {
                name: "src1".to_string(),
                agents_found: 1,
                skills_found: 0,
                warnings: vec![],
            },
            SourceScanResult {
                name: "src2".to_string(),
                agents_found: 0,
                skills_found: 3,
                warnings: vec![],
            },
        ]);
        let out = r.to_string();
        assert!(out.contains("Source 'src1'"));
        assert!(out.contains("Source 'src2'"));
    }

    // --- DiffOutput (llm feature only) ---

    #[cfg(feature = "llm")]
    #[test]
    fn diff_output_up_to_date() {
        use crate::diff::DiffOutput;
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: true,
            installed_version: None,
            source_version: None,
            source_name: "src".to_string(),
            diff_text: None,
            analysis: None,
        };
        assert!(r.to_string().contains("is up to date with source."));
    }

    #[cfg(feature = "llm")]
    #[test]
    fn diff_output_with_analysis() {
        use crate::diff::DiffOutput;
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: false,
            installed_version: Some("1.0.0".to_string()),
            source_version: Some("2.0.0".to_string()),
            source_name: "src".to_string(),
            diff_text: Some("--- a\n+++ b\n".to_string()),
            analysis: Some("Breaking changes added.".to_string()),
        };
        let out = r.to_string();
        assert!(out.contains("Comparing my-agent"));
        assert!(out.contains("Breaking changes added."));
    }

    #[cfg(feature = "llm")]
    #[test]
    fn diff_output_diff_text_only() {
        use crate::diff::DiffOutput;
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: false,
            installed_version: None,
            source_version: None,
            source_name: "src".to_string(),
            diff_text: Some("--- a\n+++ b\n".to_string()),
            analysis: None,
        };
        assert!(r.to_string().contains("Differences:"));
    }
}
