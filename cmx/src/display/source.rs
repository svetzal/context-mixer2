use std::fmt;

use crate::source::{SourceBrowseResult, SourceListResult, SourceRemoveResult, SourceScanResult};
use crate::source_update::SourceUpdateOutput;
use crate::table::section;
use crate::types::format_version_prefix;

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
            let lines: Vec<String> = self
                .agents
                .iter()
                .map(|a| {
                    let v = format_version_prefix(a.version.as_deref());
                    format!("{}{v}{}", a.name, a.deprecation_display)
                })
                .collect();
            write!(f, "{}", section("Agents:", &lines))?;
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
    use super::*;
    use crate::scan::ScanWarning;
    use crate::source::{BrowseArtifact, BrowseSkill, SourceListEntry};

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
