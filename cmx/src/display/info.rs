//! Output formatting for `cmx info`, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::info::ArtifactInfo;

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

        write!(f, "\nWhat it does:  ")?;
        if let Some(summary) = &self.summary {
            writeln!(f, "{summary}")?;
        } else if let Some(err) = &self.summary_error {
            writeln!(f, "({err})")?;
        } else {
            // No attempt was made — a lean build with no `llm` feature.
            writeln!(f, "(build cmx with `--features llm` for an LLM-generated summary)")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::SkillFileEntry;
    use crate::types::{ArtifactKind, Deprecation};
    use std::path::PathBuf;

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
            summary_error: None,
        }
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

    #[test]
    fn display_skill_shows_activates_when_and_summary() {
        let mut r = minimal_artifact_info("my-skill");
        r.kind = crate::types::ArtifactKind::Skill;
        r.activates_when = Some("Use this skill when you need X".to_string());
        r.summary = Some("It does a thing.".to_string());
        let out = r.to_string();
        assert!(out.contains("Activates when:"), "skill activation label: {out}");
        assert!(out.contains("Use this skill when you need X"));
        assert!(out.contains("What it does:"));
        assert!(out.contains("It does a thing."));
    }

    #[test]
    fn display_agent_uses_description_label() {
        let mut r = minimal_artifact_info("my-agent");
        r.activates_when = Some("A helpful agent".to_string());
        let out = r.to_string();
        assert!(out.contains("Description:"), "agent uses Description label: {out}");
        assert!(!out.contains("Activates when:"), "agent does not use the skill label");
    }

    #[test]
    fn display_summary_hint_when_no_attempt() {
        // Neither a summary nor an error: no attempt was made (a lean build).
        let r = minimal_artifact_info("my-skill");
        let out = r.to_string();
        assert!(out.contains("--features llm"), "lean build hint: {out}");
    }

    #[test]
    fn display_summary_reports_attempt_failure_reason() {
        // A summary was attempted but failed — show the real reason verbatim,
        // not a generic "provider unavailable".
        let mut r = minimal_artifact_info("productivity");
        r.summary_error =
            Some("summary unavailable — no readable content to summarize at /x.".to_string());
        let out = r.to_string();
        assert!(out.contains("no readable content to summarize"), "names real reason: {out}");
        assert!(!out.contains("--features llm"), "not the lean hint: {out}");
    }

    #[test]
    fn display_summary_gateway_failure_is_one_line_and_sanitized() {
        let mut r = minimal_artifact_info("focus-skill");
        r.summary_error = Some(
            "summary unavailable — OpenAI API error: 401 Unauthorized. Fix with 'cmx config gateway'/'cmx config model' or set OPENAI_API_KEY.".to_string(),
        );
        let out = r.to_string();
        assert!(
            out.contains(
                "What it does:  (summary unavailable — OpenAI API error: 401 Unauthorized."
            ),
            "{out}"
        );
        assert!(!out.contains("\"error\""), "{out}");
    }
}
