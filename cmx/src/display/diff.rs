#[cfg(feature = "llm")]
use crate::diff::{DiffOutput, FileStatus};
#[cfg(feature = "llm")]
use std::fmt;

#[cfg(feature = "llm")]
impl fmt::Display for DiffOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_up_to_date {
            return writeln!(f, "{} matches {}.", self.artifact_name, self.source_name);
        }

        let installed_ver = self.installed_version.as_deref().unwrap_or("unversioned");
        let source_ver = self.source_version.as_deref().unwrap_or("unversioned");
        let edited = if self.installed_locally_edited {
            ", locally edited"
        } else {
            ""
        };

        writeln!(f, "Comparing {} ({}) — copies differ", self.artifact_name, self.kind)?;
        writeln!(f)?;
        writeln!(f, "  installed  {}  ({installed_ver}{edited})", self.installed_path.display())?;
        writeln!(f, "  {}  {}  ({source_ver})", self.source_name, self.source_path.display())?;
        writeln!(f)?;

        if !self.file_changes.is_empty() {
            writeln!(f, "Changed files  (\u{2212} {}, + installed):", self.source_name)?;
            for c in &self.file_changes {
                let (flag, detail) = match c.status {
                    FileStatus::Modified => ('M', format!("+{}  \u{2212}{}", c.added, c.removed)),
                    FileStatus::OnlyInInstalled => ('A', "only in installed".to_string()),
                    FileStatus::OnlyInSource => ('D', format!("only in {}", self.source_name)),
                };
                writeln!(f, "  {flag}  {:<42}  {detail}", c.path)?;
            }
            writeln!(f)?;
        }

        if let Some(analysis) = &self.analysis {
            writeln!(f, "Summary:")?;
            writeln!(f, "{analysis}")?;
            writeln!(f)?;
        }

        if let Some(diff) = &self.diff_text {
            if !diff.is_empty() {
                writeln!(f, "Diff  (\u{2212} {}, + installed):", self.source_name)?;
                write!(f, "{diff}")?;
                writeln!(f)?;
            }
        }

        if !self.reconciliations.is_empty() {
            writeln!(f, "Reconcile — pick a direction:")?;
            for r in &self.reconciliations {
                writeln!(f, "  {:<42}  {}", r.description, r.command)?;
                if let Some(note) = &r.note {
                    writeln!(f, "      ({note})")?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(all(test, feature = "llm"))]
mod tests {
    use crate::diff::{DiffOutput, FileChange, FileStatus, Reconciliation};
    use crate::types::ArtifactKind;
    use std::path::PathBuf;

    fn diverged() -> DiffOutput {
        DiffOutput {
            artifact_name: "personal-finance".to_string(),
            kind: ArtifactKind::Skill,
            is_up_to_date: false,
            installed_path: PathBuf::from("/u/.claude/skills/personal-finance"),
            installed_version: None,
            installed_locally_edited: true,
            source_path: PathBuf::from("/u/.config/cmx/home/skills/personal-finance"),
            source_version: None,
            source_name: "home".to_string(),
            file_changes: vec![
                FileChange {
                    path: "SKILL.md".to_string(),
                    status: FileStatus::Modified,
                    added: 124,
                    removed: 6,
                },
                FileChange {
                    path: "references/new.md".to_string(),
                    status: FileStatus::OnlyInInstalled,
                    added: 10,
                    removed: 0,
                },
            ],
            diff_text: Some(
                "--- home/SKILL.md\n+++ installed/SKILL.md\n  - old\n  + new\n".to_string(),
            ),
            analysis: Some(
                "The installed copy is the newer, more authoritative rule set.".to_string(),
            ),
            reconciliations: vec![
                Reconciliation {
                    description: "keep the installed edits, update the home".to_string(),
                    command: "cmx skill promote personal-finance".to_string(),
                    note: None,
                },
                Reconciliation {
                    description: "discard the installed edits, restore the home".to_string(),
                    command: "cmx skill update personal-finance --force".to_string(),
                    note: Some("--force overwrites the installed local edits".to_string()),
                },
            ],
        }
    }

    #[test]
    fn up_to_date_message() {
        let mut r = diverged();
        r.is_up_to_date = true;
        let out = r.to_string();
        assert!(out.contains("matches home."), "got: {out}");
        assert!(!out.contains("Reconcile"), "no reconcile section when in sync");
    }

    #[test]
    fn header_names_both_sides_with_paths_and_edit_flag() {
        let out = diverged().to_string();
        assert!(out.contains("installed  /u/.claude/skills/personal-finance"), "{out}");
        assert!(out.contains("locally edited"), "flags the local edit: {out}");
        assert!(out.contains("home  /u/.config/cmx/home/skills/personal-finance"), "{out}");
    }

    #[test]
    fn file_summary_is_directional() {
        let out = diverged().to_string();
        assert!(out.contains("Changed files"), "{out}");
        assert!(out.contains("\u{2212} home, + installed"), "states the convention: {out}");
        assert!(out.contains("M  SKILL.md"), "modified flag: {out}");
        assert!(out.contains("+124"), "added count: {out}");
        assert!(out.contains("A  references/new.md"), "added-file flag: {out}");
        assert!(out.contains("only in installed"), "{out}");
    }

    #[test]
    fn shows_analysis_and_both_reconciliation_directions() {
        let out = diverged().to_string();
        assert!(out.contains("Summary:"), "{out}");
        assert!(out.contains("more authoritative"), "{out}");
        assert!(out.contains("Reconcile — pick a direction:"), "{out}");
        assert!(out.contains("cmx skill promote personal-finance"), "promote direction: {out}");
        assert!(
            out.contains("cmx skill update personal-finance --force"),
            "update direction: {out}"
        );
        assert!(out.contains("--force overwrites"), "caveat shown: {out}");
    }
}
