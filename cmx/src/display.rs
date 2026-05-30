use std::fmt;

use crate::adopt::AdoptOutcome;
use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
use crate::doctor::DoctorReport;
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
        if self.was_on_disk {
            writeln!(f, "Uninstalled {} ({}) from {} scope.", self.name, self.kind, self.scope)?;
            if !self.was_tracked {
                writeln!(f, "  (no lock file entry found — artifact was untracked)")?;
            }
        } else {
            // File was already gone — we only reconciled the stale lock entry.
            writeln!(
                f,
                "Cleared stale lock entry for {} ({}) in {} scope — the artifact was already absent from disk.",
                self.name, self.kind, self.scope
            )?;
        }
        Ok(())
    }
}

/// Build the "Installed artifacts" table from the survey rows.
fn doctor_installed_table(report: &DoctorReport) -> Table {
    Table {
        headers: vec!["Type", "Name", "Scope", "State", "Version", "Location"],
        padded_cols: 5,
        rows: report
            .rows
            .iter()
            .map(|r| {
                let mut cells = vec![
                    r.kind.to_string(),
                    r.name.clone(),
                    r.scope.label().to_string(),
                    r.state.label().to_string(),
                    r.version.clone().unwrap_or_else(|| "-".to_string()),
                    r.location.display().to_string(),
                ];
                if r.duplicated {
                    cells.push("(dup)".to_string());
                }
                cells
            })
            .collect(),
    }
}

/// Build the "Missing" table from lock entries with no file on disk.
fn doctor_missing_table(report: &DoctorReport) -> Table {
    Table {
        headers: vec!["Type", "Name", "Scope", "Platform"],
        padded_cols: 4,
        rows: report
            .missing
            .iter()
            .map(|m| {
                vec![
                    m.kind.to_string(),
                    m.name.clone(),
                    m.scope.label().to_string(),
                    m.platform.to_string(),
                ]
            })
            .collect(),
    }
}

/// Honest next-step hints — one line per state that actually occurs, referring
/// only to capabilities that exist today.
fn doctor_hints(c: &crate::doctor::StateCounts) -> String {
    let mut lines = Vec::new();
    if c.orphaned > 0 {
        lines.push(format!(
            "  • {} orphaned artifact(s) are not tracked by cmx (no source, no lock entry).",
            c.orphaned
        ));
    }
    if c.drifted > 0 {
        lines.push(format!(
            "  • {} drifted artifact(s) differ from their lock file — inspect with `cmx info <name>`.",
            c.drifted
        ));
    }
    if c.missing > 0 {
        lines.push(format!(
            "  • {} missing artifact(s) are recorded in a lock file but gone from disk — `cmx <kind> uninstall <name>` clears the stale entry (or reinstall if the source still has it).",
            c.missing
        ));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", lines.join("\n"))
    }
}

impl fmt::Display for AdoptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.adopted.is_empty() {
            return writeln!(
                f,
                "Nothing to adopt — no orphaned artifacts found{}.",
                if self.included_local {
                    " (global + project scope)"
                } else {
                    ""
                }
            );
        }
        writeln!(f, "Adopted {} artifact(s) into {}:", self.adopted.len(), self.home.display())?;
        for a in &self.adopted {
            let tools: Vec<String> = a.platforms.iter().map(ToString::to_string).collect();
            writeln!(f, "  {} {} — now tracked for: {}", a.kind, a.name, tools.join(", "))?;
        }
        writeln!(f)?;
        writeln!(
            f,
            "Project them to another tool with: cmx skill install --all --platform <tool>"
        )
    }
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scope_desc = if self.included_local {
            "global + project scope"
        } else {
            "global scope"
        };
        writeln!(
            f,
            "cmx doctor — {scope_desc}, {} platforms surveyed.\n",
            crate::platform::Platform::ALL.len()
        )?;

        if self.rows.is_empty() && self.missing.is_empty() {
            return writeln!(f, "Nothing installed — your system is clean.");
        }

        if !self.rows.is_empty() {
            writeln!(f, "Installed artifacts:")?;
            write!(f, "{}", doctor_installed_table(self).render())?;
        }

        if !self.missing.is_empty() {
            if !self.rows.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Missing (in a lock file, absent on disk):")?;
            write!(f, "{}", doctor_missing_table(self).render())?;
        }

        let c = self.counts();
        writeln!(
            f,
            "\nSummary: {} tracked, {} drifted, {} orphaned, {} missing · {} duplicated across locations.",
            c.tracked, c.drifted, c.orphaned, c.missing, c.duplicated
        )?;
        write!(f, "{}", doctor_hints(&c))
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
    use crate::cmx_config::{ConfigSetResult, ConfigShowResult};
    use crate::info::{ArtifactInfo, SkillFileEntry};
    use crate::install::{BatchInstallResult, InstallResult};
    use crate::list::{ListKindOutput, ListOutput, Row};
    use crate::outdated::{OutdatedReport, OutdatedRow};
    use crate::scan::ScanWarning;
    use crate::search::{SearchOutput, SearchResult};
    use crate::source::{
        BrowseArtifact, BrowseSkill, SourceBrowseResult, SourceListEntry, SourceListResult,
        SourceRemoveResult, SourceScanResult,
    };
    use crate::source_update::SourceUpdateOutput;
    use crate::types::{ArtifactKind, Deprecation, InstallScope};
    use crate::uninstall::UninstallResult;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn make_row(name: &str) -> Row {
        Row {
            name: name.to_string(),
            installed: "1.0.0".to_string(),
            source: "src".to_string(),
            available: "1.0.0".to_string(),
            status: "✅",
        }
    }

    fn minimal_artifact_info(name: &str) -> ArtifactInfo {
        ArtifactInfo {
            name: name.to_string(),
            kind: ArtifactKind::Agent,
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

    // --- Step 1: SourceListResult ---

    #[test]
    fn source_list_result_empty_shows_hint() {
        let r = SourceListResult { entries: vec![] };
        let out = r.to_string();
        assert!(out.contains("No sources registered."));
        assert!(out.contains("cmx source add"));
    }

    #[test]
    fn source_list_result_populated_shows_name_kind_location() {
        let r = SourceListResult {
            entries: vec![SourceListEntry {
                name: "my-source".to_string(),
                kind: "local",
                location: "/repos/my-source".to_string(),
            }],
        };
        let out = r.to_string();
        assert!(out.contains("my-source"));
        assert!(out.contains("local"));
        assert!(out.contains("/repos/my-source"));
    }

    // --- Step 2: SourceBrowseResult ---

    #[test]
    fn source_browse_result_empty_source() {
        let r = SourceBrowseResult {
            source_name: "empty-src".to_string(),
            agents: vec![],
            skills: vec![],
        };
        assert!(r.to_string().contains("No agents or skills found in 'empty-src'"));
    }

    #[test]
    fn source_browse_result_agents_only_shows_agents_header() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
            agents: vec![BrowseArtifact {
                name: "my-agent".to_string(),
                version: Some("1.0.0".to_string()),
                deprecation_display: String::new(),
            }],
            skills: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("my-agent"));
        assert!(!out.contains("Skills:"));
    }

    #[test]
    fn source_browse_result_both_sections_skill_files_indented() {
        let r = SourceBrowseResult {
            source_name: "src".to_string(),
            agents: vec![BrowseArtifact {
                name: "agent-x".to_string(),
                version: None,
                deprecation_display: String::new(),
            }],
            skills: vec![BrowseSkill {
                name: "skill-y".to_string(),
                version: None,
                deprecation_display: String::new(),
                files: vec!["tool.md".to_string()],
            }],
        };
        let out = r.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("    tool.md"));
    }

    // --- Step 3: SourceScanResult ---

    #[test]
    fn source_scan_result_no_warnings_single_line() {
        let r = SourceScanResult {
            name: "src".to_string(),
            agents_found: 2,
            skills_found: 3,
            warnings: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("src"));
        assert!(out.contains("2 agent(s)"));
        assert!(out.contains("3 skill(s)"));
        assert!(!out.contains("Warning:"));
    }

    #[test]
    fn source_scan_result_warnings_appended() {
        let r = SourceScanResult {
            name: "src".to_string(),
            agents_found: 0,
            skills_found: 0,
            warnings: vec![ScanWarning {
                message: "bad frontmatter".to_string(),
            }],
        };
        assert!(r.to_string().contains("Warning: bad frontmatter"));
    }

    // --- Step 4: SourceRemoveResult ---

    #[test]
    fn source_remove_result_no_clone() {
        let r = SourceRemoveResult {
            name: "local-src".to_string(),
            clone_deleted: false,
        };
        let out = r.to_string();
        assert!(out.contains("local-src"));
        assert!(out.contains("removed."));
        assert!(!out.contains("cloned repo deleted"));
    }

    #[test]
    fn source_remove_result_clone_deleted() {
        let r = SourceRemoveResult {
            name: "git-src".to_string(),
            clone_deleted: true,
        };
        assert!(r.to_string().contains("cloned repo deleted"));
    }

    // --- Step 5: InstallResult and BatchInstallResult ---

    #[test]
    fn install_result_with_version_includes_version_prefix() {
        let r = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.claude/agents"),
            version: Some("1.2.3".to_string()),
        };
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("v1.2.3"));
        assert!(out.contains("guidelines"));
    }

    #[test]
    fn batch_install_result_empty_update_up_to_date() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: true,
        };
        assert!(r.to_string().contains("up to date"));
    }

    #[test]
    fn batch_install_result_empty_install_already_installed() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        assert!(r.to_string().contains("already installed"));
    }

    #[test]
    fn batch_install_result_with_items_delegates_to_install_result() {
        let r = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-skill".to_string(),
                kind: ArtifactKind::Skill,
                source_name: "src".to_string(),
                dest_dir: PathBuf::from("/home/user/.claude/skills"),
                version: None,
            }],
            kind: ArtifactKind::Skill,
            is_update: false,
        };
        let out = r.to_string();
        assert!(out.contains("my-skill"));
        assert!(out.contains("src"));
    }

    // --- Step 6: UninstallResult ---

    #[test]
    fn uninstall_result_tracked_single_line() {
        let r = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: true,
            was_on_disk: true,
        };
        let out = r.to_string();
        assert!(out.contains("Uninstalled my-agent"));
        assert!(!out.contains("untracked"));
    }

    #[test]
    fn uninstall_result_untracked_includes_note() {
        let r = UninstallResult {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: "global",
            was_tracked: false,
            was_on_disk: true,
        };
        assert!(r.to_string().contains("untracked"));
    }

    #[test]
    fn uninstall_result_reconciled_missing_entry() {
        // File already gone, only the stale lock entry was cleared.
        let r = UninstallResult {
            name: "skill-writing".to_string(),
            kind: ArtifactKind::Skill,
            scope: "global",
            was_tracked: true,
            was_on_disk: false,
        };
        let out = r.to_string();
        assert!(out.contains("Cleared stale lock entry for skill-writing"), "got: {out}");
        assert!(out.contains("already absent from disk"));
        assert!(!out.contains("Uninstalled"), "should not claim a real uninstall");
    }

    // --- Step 7: ListKindOutput and ListOutput ---

    #[test]
    fn list_kind_output_empty_shows_none_installed() {
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows: BTreeMap::new(),
        };
        assert_eq!(r.to_string(), "No agents installed.\n");
    }

    #[test]
    fn list_kind_output_global_only_shows_global_header() {
        let mut rows = BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("agent-a")]);
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows,
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("agent-a"));
    }

    #[test]
    fn list_kind_output_both_scopes_shows_both_headers() {
        let mut rows = BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("agent-g")]);
        rows.insert(InstallScope::Local, vec![make_row("agent-l")]);
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows,
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("Local agents:"));
    }

    #[test]
    fn list_output_empty_shows_nothing_installed() {
        let r = ListOutput {
            agents: BTreeMap::new(),
            skills: BTreeMap::new(),
        };
        assert_eq!(r.to_string(), "Nothing installed.\n");
    }

    #[test]
    fn list_output_with_agents_shows_section() {
        let mut agents = BTreeMap::new();
        agents.insert(InstallScope::Global, vec![make_row("my-agent")]);
        let r = ListOutput {
            agents,
            skills: BTreeMap::new(),
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("my-agent"));
    }

    // --- Step 8: OutdatedReport ---

    #[test]
    fn outdated_report_empty_up_to_date() {
        let r = OutdatedReport(vec![]);
        assert_eq!(r.to_string(), "Everything is up to date.\n");
    }

    #[test]
    fn outdated_report_populated_shows_rows() {
        let r = OutdatedReport(vec![OutdatedRow {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            installed_version: "1.0.0".to_string(),
            available_version: "2.0.0".to_string(),
            source: "guidelines".to_string(),
            status: "update".to_string(),
        }]);
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("2.0.0"));
    }

    // --- Step 9: SearchOutput ---

    #[test]
    fn search_output_empty_no_results_message() {
        let r = SearchOutput {
            query: "my-query".to_string(),
            results: vec![],
        };
        assert_eq!(r.to_string(), "No results for 'my-query'.\n");
    }

    #[test]
    fn search_output_populated_result_count() {
        let r = SearchOutput {
            query: "rust".to_string(),
            results: vec![SearchResult {
                name: "rust-craftsperson".to_string(),
                kind: "agent".to_string(),
                version: "1.0.0".to_string(),
                source: "guidelines".to_string(),
                description: "Rust expert".to_string(),
            }],
        };
        let out = r.to_string();
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("1 result(s) found."));
    }

    // --- Step 10: ArtifactInfo ---

    #[test]
    fn artifact_info_minimal_shows_required_fields() {
        let r = minimal_artifact_info("my-agent");
        let out = r.to_string();
        assert!(out.contains("Name:        my-agent"));
        assert!(out.contains("Type:        agent"));
        assert!(out.contains("Scope:       global"));
        assert!(out.contains("Path:"));
    }

    #[test]
    fn artifact_info_all_optional_fields_rendered() {
        let mut r = minimal_artifact_info("my-agent");
        r.version = Some("1.0.0".to_string());
        r.installed_at = Some("2024-01-01T00:00:00Z".to_string());
        r.source_display = Some("guidelines (my-agent.md)".to_string());
        r.source_checksum = Some("sha256:source".to_string());
        r.installed_checksum = Some("sha256:installed".to_string());
        r.available_version = Some("2.0.0".to_string());
        r.deprecation = Some(Deprecation {
            reason: Some("obsolete".to_string()),
            replacement: Some("new-agent".to_string()),
        });
        r.skill_files = vec![SkillFileEntry {
            name: "SKILL.md".to_string(),
            is_dir: false,
            indent_level: 0,
        }];
        let out = r.to_string();
        assert!(out.contains("Version:     1.0.0"));
        assert!(out.contains("Installed:   2024-01-01T00:00:00Z"));
        assert!(out.contains("Source:      guidelines (my-agent.md)"));
        assert!(out.contains("DEPRECATED"));
        assert!(out.contains("obsolete"));
        assert!(out.contains("new-agent"));
        assert!(out.contains("v2.0.0"));
        assert!(out.contains("SKILL.md"));
    }

    #[test]
    fn artifact_info_locally_modified_suffix() {
        let mut r = minimal_artifact_info("my-agent");
        r.locally_modified = true;
        r.disk_checksum = Some("sha256:disk".to_string());
        assert!(r.to_string().contains("(locally modified)"));
    }

    #[test]
    fn artifact_info_untracked_note() {
        let mut r = minimal_artifact_info("my-agent");
        r.untracked = true;
        assert!(r.to_string().contains("untracked"));
    }

    // --- Step 11: DiffOutput (feature-gated) ---

    #[cfg(feature = "llm")]
    #[test]
    fn diff_output_is_up_to_date_message() {
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
    fn diff_output_with_analysis_shows_analyzing() {
        use crate::diff::DiffOutput;
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: false,
            installed_version: Some("1.0.0".to_string()),
            source_version: Some("2.0.0".to_string()),
            source_name: "src".to_string(),
            diff_text: Some("--- a\n+++ b\n".to_string()),
            analysis: Some("Notable changes found.".to_string()),
        };
        let out = r.to_string();
        assert!(out.contains("Analyzing differences..."));
        assert!(out.contains("Notable changes found."));
    }

    #[cfg(feature = "llm")]
    #[test]
    fn diff_output_diff_text_only_shows_differences() {
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

    // --- Step 12: ConfigShowResult and ConfigSetResult ---

    #[test]
    fn config_show_result_contains_gateway_and_model_labels() {
        let r = ConfigShowResult {
            gateway: "ollama".to_string(),
            model: "llama3".to_string(),
        };
        let out = r.to_string();
        assert!(out.contains("LLM gateway:"));
        assert!(out.contains("LLM model:"));
    }

    #[test]
    fn config_set_result_contains_field_and_value() {
        let r = ConfigSetResult {
            field: "model",
            value: "gpt-4".to_string(),
        };
        let out = r.to_string();
        assert!(out.contains("model"));
        assert!(out.contains("gpt-4"));
    }

    // --- AdoptOutcome ---

    #[test]
    fn adopt_outcome_empty_message() {
        let o = crate::adopt::AdoptOutcome {
            adopted: vec![],
            home: PathBuf::from("/home/u/.config/context-mixer/home"),
            included_local: false,
        };
        assert!(o.to_string().contains("Nothing to adopt"));
    }

    #[test]
    fn adopt_outcome_lists_adopted_and_projection_hint() {
        let o = crate::adopt::AdoptOutcome {
            adopted: vec![crate::adopt::AdoptResult {
                kind: ArtifactKind::Skill,
                name: "my-skill".to_string(),
                home_path: PathBuf::from("/home/u/.config/context-mixer/home/skills/my-skill"),
                platforms: vec![crate::platform::Platform::Claude],
            }],
            home: PathBuf::from("/home/u/.config/context-mixer/home"),
            included_local: false,
        };
        let out = o.to_string();
        assert!(out.contains("Adopted 1 artifact(s)"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("now tracked for: claude"));
        assert!(out.contains("install --all --platform"), "projection hint present");
    }

    // --- DoctorReport ---

    fn orphan_row(name: &str) -> crate::doctor::DoctorRow {
        crate::doctor::DoctorRow {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            location: PathBuf::from("/home/u/.claude/skills"),
            platforms: vec![crate::platform::Platform::Claude],
            state: crate::doctor::ArtifactState::Orphaned,
            version: Some("1.0.0".to_string()),
            duplicated: false,
        }
    }

    #[test]
    fn doctor_report_clean_system_message() {
        let r = crate::doctor::DoctorReport::default();
        let out = r.to_string();
        assert!(out.contains("Nothing installed"), "clean message: {out}");
        assert!(out.contains("global scope"), "default scope description");
    }

    #[test]
    fn doctor_report_lists_rows_and_summary() {
        let r = crate::doctor::DoctorReport {
            rows: vec![orphan_row("my-skill")],
            missing: vec![],
            included_local: false,
        };
        let out = r.to_string();
        assert!(out.contains("Installed artifacts:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("orphaned"));
        assert!(out.contains("1 orphaned"), "summary tallies orphans: {out}");
        assert!(out.contains("not tracked by cmx"), "orphan hint present");
    }

    #[test]
    fn doctor_report_missing_section_and_scope_label() {
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            missing: vec![crate::doctor::MissingRow {
                kind: ArtifactKind::Skill,
                name: "ghost".to_string(),
                scope: InstallScope::Global,
                platform: crate::platform::Platform::Pi,
            }],
            included_local: true,
        };
        let out = r.to_string();
        assert!(out.contains("global + project scope"), "local-included scope label");
        assert!(out.contains("Missing (in a lock file"));
        assert!(out.contains("ghost"));
        assert!(out.contains("pi"));
        assert!(out.contains("1 missing"));
    }

    #[test]
    fn doctor_report_marks_duplicated_rows() {
        let mut row = orphan_row("dup");
        row.duplicated = true;
        let r = crate::doctor::DoctorReport {
            rows: vec![row],
            missing: vec![],
            included_local: false,
        };
        let out = r.to_string();
        assert!(out.contains("(dup)"), "duplicated marker rendered: {out}");
        assert!(out.contains("1 duplicated across locations"));
    }

    // --- Step 13: SourceUpdateOutput ---

    #[test]
    fn source_update_output_no_git_sources() {
        assert!(
            SourceUpdateOutput::NoGitSources
                .to_string()
                .contains("No git-backed sources to update.")
        );
    }

    #[test]
    fn source_update_output_single_update_shows_counts() {
        let r = SourceUpdateOutput::SingleUpdate(SourceScanResult {
            name: "guidelines".to_string(),
            agents_found: 4,
            skills_found: 2,
            warnings: vec![],
        });
        let out = r.to_string();
        assert!(out.contains("guidelines"));
        assert!(out.contains("4 agent(s)"));
    }

    #[test]
    fn source_update_output_batch_shows_multiple_lines() {
        let r = SourceUpdateOutput::BatchUpdate(vec![
            SourceScanResult {
                name: "src-a".to_string(),
                agents_found: 1,
                skills_found: 0,
                warnings: vec![],
            },
            SourceScanResult {
                name: "src-b".to_string(),
                agents_found: 0,
                skills_found: 2,
                warnings: vec![],
            },
        ]);
        let out = r.to_string();
        assert!(out.contains("src-a"));
        assert!(out.contains("src-b"));
    }
}
