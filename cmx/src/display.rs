use std::fmt;

use crate::adopt::{AdoptOutcome, UnadoptOutcome};
use crate::cmx_config::{ConfigSetResult, ConfigShowResult, ExternalResult};
#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
use crate::doctor::DoctorReport;
use crate::info::ArtifactInfo;
use crate::install::{BatchInstallResult, InstallManyResult, InstallResult};
use crate::list::{ListKindOutput, ListOutput, section_str, table_str};
use crate::outdated::OutdatedReport;
use crate::search::SearchOutput;
use crate::source::{SourceBrowseResult, SourceListResult, SourceRemoveResult, SourceScanResult};
use crate::source_update::SourceUpdateOutput;
use crate::table::{Table, render_table};
use crate::types::{InstallScope, format_version_prefix};
use crate::uninstall::{BatchUninstallResult, UninstallResult};

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

impl fmt::Display for InstallManyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.installed {
            write!(f, "{r}")?;
        }
        for (name, reason) in &self.failed {
            writeln!(f, "Failed: {name} — {reason}")?;
        }
        if self.installed.is_empty() && self.failed.is_empty() {
            writeln!(f, "No {}s given to install.", self.kind)?;
        }
        Ok(())
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
            render_table(
                vec!["Name", "Type", "Installed", "Available", "Source", "Status"],
                6,
                rows.iter()
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
            )
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

        let table = render_table(
            vec!["Name", "Type", "Version", "Source", "Description"],
            4,
            results
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
        );

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

        if let Some(desc) = &self.activates_when {
            // For a skill the description *is* the activation trigger; for an
            // agent it's a role description.
            let header = match self.kind {
                crate::types::ArtifactKind::Skill => "Activates when:",
                crate::types::ArtifactKind::Agent => "Description:",
            };
            writeln!(f, "\n{header}")?;
            writeln!(f, "  {desc}")?;
        }

        match &self.summary {
            Some(summary) => {
                writeln!(f, "\nWhat it does:")?;
                writeln!(f, "  {summary}")?;
            }
            None => {
                writeln!(
                    f,
                    "\nWhat it does:\n  (build cmx with `--features llm` for an LLM-generated summary)"
                )?;
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
        let tools = self.platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        if self.was_on_disk {
            writeln!(f, "Uninstalled {} ({}) from {} scope.", self.name, self.kind, self.scope)?;
            if self.was_tracked {
                writeln!(f, "  Cleared lock entries for: {tools}")?;
            } else {
                writeln!(f, "  (no lock file entry found — artifact was untracked)")?;
            }
        } else {
            // Files were already gone — we only reconciled stale lock entries.
            writeln!(
                f,
                "Cleared stale lock entry for {} ({}) in {} scope ({tools}) — the artifact was already absent from disk.",
                self.name, self.kind, self.scope
            )?;
        }
        Ok(())
    }
}

/// Build the artifact table from the given grouped logical artifacts — one row
/// per skill, the Tools column listing every tool it's installed for.
fn doctor_artifact_table(artifacts: &[&crate::doctor::DoctorArtifact]) -> Table {
    Table {
        headers: vec![
            "Type", "Name", "Scope", "State", "Version", "Source", "Tools",
        ],
        padded_cols: 6,
        rows: artifacts
            .iter()
            .map(|a| {
                let tools = if a.tools.is_empty() {
                    "-".to_string()
                } else {
                    a.tools.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
                };
                // When copies agree show the single version; when they diverge
                // name the skew (`3.2.0 / 3.3.0`) rather than an opaque `-`.
                let version = a.version.clone().unwrap_or_else(|| {
                    if a.versions.len() > 1 {
                        a.versions.join(" / ")
                    } else {
                        "-".to_string()
                    }
                });
                let mut cells = vec![
                    a.kind.to_string(),
                    a.name.clone(),
                    a.scope.label().to_string(),
                    a.state.label().to_string(),
                    version,
                    a.source.clone().unwrap_or_else(|| "-".to_string()),
                    tools,
                ];
                if a.diverged {
                    cells.push("(diverged)".to_string());
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
            "  • {} orphaned artifact(s) have no source (hand-authored) — `cmx <kind> adopt <name>` (or `cmx doctor --adopt-all`) canonicalizes them into the home.",
            c.orphaned
        ));
    }
    if c.untracked > 0 {
        lines.push(format!(
            "  • {} untracked artifact(s) are installed but a registered source provides them — `cmx <kind> install <name>` records provenance and tracks them.",
            c.untracked
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
    if c.diverged > 0 {
        lines.push(format!(
            "  • {} artifact(s) diverge across their install locations (different version or state). Re-sync a cmx-managed one with `cmx <kind> update <name> --force`; an external one is the owning tool's to re-sync.",
            c.diverged
        ));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", lines.join("\n"))
    }
}

/// Per-location breakdown for each shown diverged artifact, naming which copy
/// carries which version (and state, when states differ too). Built from the raw
/// rows — which pair location↔version — because the grouped table collapses them.
fn doctor_divergence_details(
    shown: &[&crate::doctor::DoctorArtifact],
    rows: &[crate::doctor::DoctorRow],
) -> String {
    let mut lines = Vec::new();
    for a in shown.iter().filter(|a| a.diverged) {
        let mut members: Vec<&crate::doctor::DoctorRow> = rows
            .iter()
            .filter(|r| r.kind == a.kind && r.name == a.name && r.scope == a.scope)
            .collect();
        members.sort_by(|x, y| x.location.cmp(&y.location));
        let states_differ = members
            .iter()
            .map(|r| r.state.label())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1;
        let parts: Vec<String> = members
            .iter()
            .map(|r| {
                let ver = r.version.as_deref().unwrap_or("unversioned");
                if states_differ {
                    format!("{} @ {ver} ({})", r.location.display(), r.state.label())
                } else {
                    format!("{} @ {ver}", r.location.display())
                }
            })
            .collect();
        lines.push(format!("  • {} diverges: {}", a.name, parts.join(", ")));
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

impl fmt::Display for UnadoptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.unadopted {
            write!(f, "Unadopted {} ({}) — removed from the home", r.name, r.kind)?;
            if r.untracked_from.is_empty() {
                writeln!(f, ".")?;
            } else {
                let tools =
                    r.untracked_from.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
                writeln!(f, "; un-tracked for: {tools}.")?;
            }
        }
        if !self.not_adopted.is_empty() {
            writeln!(f, "Not adopted (nothing in the home): {}", self.not_adopted.join(", "))?;
        }
        if !self.unadopted.is_empty() {
            writeln!(
                f,
                "\nThe on-disk copies remain (now orphaned). Mark them external with \
                 `cmx config external add <name>` if another tool manages them."
            )?;
        }
        Ok(())
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

        // By default `doctor` shows only what needs attention — it's a doctor.
        // `--all` shows the full inventory.
        let shown: Vec<&crate::doctor::DoctorArtifact> = if self.show_all {
            self.artifacts.iter().collect()
        } else {
            self.artifacts.iter().filter(|a| DoctorReport::is_problem(a)).collect()
        };

        if !shown.is_empty() {
            writeln!(
                f,
                "{}",
                if self.show_all {
                    "Installed artifacts:"
                } else {
                    "Needs attention:"
                }
            )?;
            write!(f, "{}", doctor_artifact_table(&shown).render())?;
        }

        if !self.missing.is_empty() {
            if !shown.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Missing (in a lock file, absent on disk):")?;
            write!(f, "{}", doctor_missing_table(self).render())?;
        }

        if shown.is_empty() && self.missing.is_empty() {
            if self.artifacts.is_empty() {
                writeln!(f, "Nothing installed — your system is clean.")?;
            } else if self.show_all {
                writeln!(f, "No artifacts found.")?;
            } else {
                writeln!(
                    f,
                    "No problems — everything cmx manages is healthy. (`--all` shows the full inventory.)"
                )?;
            }
        }

        let c = self.counts();
        writeln!(
            f,
            "\nSummary: {} tracked, {} drifted, {} untracked, {} orphaned, {} external, {} missing · {} diverged.",
            c.tracked, c.drifted, c.untracked, c.orphaned, c.external, c.missing, c.diverged
        )?;
        write!(f, "{}", doctor_hints(&c))?;
        write!(f, "{}", doctor_divergence_details(&shown, &self.rows))
    }
}

impl fmt::Display for BatchUninstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.removed {
            write!(f, "{r}")?;
        }
        if !self.not_found.is_empty() {
            writeln!(f, "Not found (nothing to uninstall): {}", self.not_found.join(", "))?;
        }
        if self.removed.is_empty() && self.not_found.is_empty() {
            writeln!(f, "No {}s given to uninstall.", self.kind)?;
        }
        Ok(())
    }
}

impl fmt::Display for ConfigShowResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLM gateway: {}\nLLM model:   {}\n", self.gateway, self.model)?;
        if self.external.is_empty() {
            writeln!(f, "External:    (none)")
        } else {
            writeln!(f, "External:    {}", self.external.join(", "))
        }
    }
}

impl fmt::Display for ExternalResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(entry) = &self.entry {
            writeln!(f, "{} external rule: {entry}", self.action)?;
        }
        if self.external.is_empty() {
            writeln!(f, "External rules: (none)")
        } else {
            writeln!(f, "External rules:")?;
            for e in &self.external {
                writeln!(f, "  {e}")?;
            }
            Ok(())
        }
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
    use crate::cmx_config::{ConfigSetResult, ConfigShowResult, ExternalResult};
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
            available: "1.0.0".to_string(),
            source: "src".to_string(),
            tools: "claude".to_string(),
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
            activates_when: None,
            summary: None,
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
            platforms: vec![crate::platform::Platform::Claude],
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
            platforms: vec![],
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
            platforms: vec![crate::platform::Platform::Claude],
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
            external: vec!["~/.hermes/skills".to_string()],
        };
        let out = r.to_string();
        assert!(out.contains("LLM gateway:"));
        assert!(out.contains("LLM model:"));
        assert!(out.contains("External:    ~/.hermes/skills"));
    }

    #[test]
    fn external_result_list_and_mutation_render() {
        let list = ExternalResult {
            action: "External rules",
            entry: None,
            external: vec!["~/.hermes/skills".to_string()],
        };
        let out = list.to_string();
        assert!(out.contains("External rules:"));
        assert!(out.contains("~/.hermes/skills"));

        let added = ExternalResult {
            action: "Added",
            entry: Some("apple".to_string()),
            external: vec!["apple".to_string()],
        };
        assert!(added.to_string().contains("Added external rule: apple"));

        let empty = ExternalResult {
            action: "External rules",
            entry: None,
            external: vec![],
        };
        assert!(empty.to_string().contains("(none)"));
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

    fn orphan_artifact(name: &str) -> crate::doctor::DoctorArtifact {
        crate::doctor::DoctorArtifact {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            state: crate::doctor::ArtifactState::Orphaned,
            version: Some("1.0.0".to_string()),
            versions: vec!["1.0.0".to_string()],
            tools: vec![crate::platform::Platform::Claude],
            source: None,
            locations: vec![PathBuf::from("/home/u/.claude/skills")],
            diverged: false,
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
    fn doctor_report_lists_artifacts_and_summary() {
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![orphan_artifact("my-skill")],
            missing: vec![],
            included_local: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("Installed artifacts:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("orphaned"));
        assert!(out.contains("1 orphaned"), "summary tallies orphans: {out}");
        assert!(out.contains("have no source"), "orphan hint present");
        assert!(out.contains("adopt"), "orphan hint points at adopt");
    }

    #[test]
    fn doctor_report_lists_tools_for_multi_tool_artifact() {
        // One skill installed for two tools is ONE row listing both — not "dup".
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![crate::doctor::DoctorArtifact {
                kind: ArtifactKind::Skill,
                name: "clipboard".to_string(),
                scope: InstallScope::Global,
                state: crate::doctor::ArtifactState::Tracked,
                version: Some("1.0.0".to_string()),
                versions: vec!["1.0.0".to_string()],
                tools: vec![
                    crate::platform::Platform::Claude,
                    crate::platform::Platform::Codex,
                ],
                source: Some("home".to_string()),
                locations: vec![PathBuf::from("/a"), PathBuf::from("/b")],
                diverged: false,
            }],
            missing: vec![],
            included_local: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("clipboard"));
        assert!(out.contains("claude, codex"), "tools listed in one row: {out}");
        assert!(out.contains("home"), "source provenance shown: {out}");
        assert!(!out.contains("(diverged)"), "consistent copies carry no diverged marker");
        assert!(out.contains("1 tracked"), "counted once, not per-location");
    }

    #[test]
    fn doctor_report_missing_section_and_scope_label() {
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![],
            missing: vec![crate::doctor::MissingRow {
                kind: ArtifactKind::Skill,
                name: "ghost".to_string(),
                scope: InstallScope::Global,
                platform: crate::platform::Platform::Pi,
            }],
            included_local: true,
            show_all: false,
        };
        let out = r.to_string();
        assert!(out.contains("global + project scope"), "local-included scope label");
        assert!(out.contains("Missing (in a lock file"));
        assert!(out.contains("ghost"));
        assert!(out.contains("pi"));
        assert!(out.contains("1 missing"));
    }

    #[test]
    fn doctor_report_flags_diverged_artifact() {
        let mut a = orphan_artifact("skew");
        a.diverged = true;
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            show_all: false,
        };
        let out = r.to_string();
        assert!(out.contains("(diverged)"), "diverged marker rendered: {out}");
        assert!(out.contains("1 diverged"));
        assert!(out.contains("diverge across their install locations"), "diverged hint present");
    }

    #[test]
    fn doctor_report_names_version_skew() {
        // A version-diverged artifact: no single agreed version, but the distinct
        // versions are shown (`3.2.0 / 3.3.0`) rather than an opaque `-`.
        let mut a = orphan_artifact("hopper-coordinator");
        a.diverged = true;
        a.version = None;
        a.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("3.2.0 / 3.3.0"), "version skew named in the table: {out}");
        assert!(out.contains("(diverged)"), "still flagged diverged: {out}");
    }

    #[test]
    fn doctor_details_name_each_locations_version() {
        use crate::doctor::{ArtifactState, DoctorRow};
        use crate::platform::Platform;

        let mk_row = |loc: &str, ver: &str, platform| DoctorRow {
            kind: ArtifactKind::Skill,
            name: "hopper-coordinator".to_string(),
            scope: InstallScope::Global,
            location: PathBuf::from(loc),
            platforms: vec![platform],
            tracked_for: vec![],
            state: ArtifactState::External,
            version: Some(ver.to_string()),
            source: None,
        };
        let mut art = orphan_artifact("hopper-coordinator");
        art.state = ArtifactState::External;
        art.diverged = true;
        art.version = None;
        art.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = crate::doctor::DoctorReport {
            rows: vec![
                mk_row("/u/.claude/skills", "3.3.0", Platform::Claude),
                mk_row("/u/.agents/skills", "3.2.0", Platform::Codex),
            ],
            artifacts: vec![art],
            missing: vec![],
            included_local: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("hopper-coordinator diverges:"), "detail line present: {out}");
        assert!(out.contains("/u/.claude/skills @ 3.3.0"), "claude copy version: {out}");
        assert!(out.contains("/u/.agents/skills @ 3.2.0"), "agents copy version: {out}");
    }

    #[test]
    fn doctor_default_view_surfaces_external_divergence() {
        // A diverged external artifact is an anomaly: the default (problems-only)
        // view must surface it rather than claim everything is healthy.
        let mut a = orphan_artifact("hopper-coordinator");
        a.state = crate::doctor::ArtifactState::External;
        a.diverged = true;
        a.version = None;
        a.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            show_all: false,
        };
        assert!(r.has_issues(), "a diverged external artifact is an issue");
        let out = r.to_string();
        assert!(out.contains("Needs attention:"), "surfaced in default view: {out}");
        assert!(out.contains("hopper-coordinator"), "the diverged artifact is shown: {out}");
        assert!(
            !out.contains("everything cmx manages is healthy"),
            "must not claim healthy while diverged: {out}"
        );
        assert!(out.contains("1 diverged"), "tally counts it: {out}");
    }

    #[test]
    fn doctor_report_problems_only_by_default() {
        // Default view (show_all=false): a tracked artifact is hidden; only
        // problems surface. With nothing wrong, a healthy message shows.
        let healthy = crate::doctor::DoctorReport {
            rows: vec![],
            artifacts: vec![crate::doctor::DoctorArtifact {
                kind: ArtifactKind::Skill,
                name: "clipboard".to_string(),
                scope: InstallScope::Global,
                state: crate::doctor::ArtifactState::Tracked,
                version: Some("1.0.0".to_string()),
                versions: vec!["1.0.0".to_string()],
                tools: vec![crate::platform::Platform::Claude],
                source: Some("home".to_string()),
                locations: vec![PathBuf::from("/a")],
                diverged: false,
            }],
            missing: vec![],
            included_local: false,
            show_all: false,
        };
        let out = healthy.to_string();
        assert!(!out.contains("clipboard"), "tracked artifact hidden by default: {out}");
        assert!(out.contains("everything cmx manages is healthy"), "healthy message: {out}");
        assert!(out.contains("1 tracked"), "summary still tallies it");
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
