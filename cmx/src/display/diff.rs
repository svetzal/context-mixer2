#[cfg(feature = "llm")]
use crate::diff::{DiffOutput, FileStatus};
#[cfg(feature = "llm")]
use std::fmt;

#[cfg(feature = "llm")]
fn platforms_label(platforms: &[crate::platform::Platform]) -> String {
    platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(feature = "llm")]
impl DiffOutput {
    /// Render the "which copy" header: a per-platform matrix when several copies
    /// exist (so a match on one platform can't mask a drift on another), or the
    /// simple installed/source pair for a single copy.
    fn write_header(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let edited = if self.installed_locally_edited {
            ", locally edited"
        } else {
            ""
        };
        if self.copies.len() > 1 {
            for c in &self.copies {
                let status = if c.matches {
                    format!("matches {}", self.source_name)
                } else {
                    format!("differs  (+{}  \u{2212}{})", c.added, c.removed)
                };
                let marker = if c.is_focus {
                    "   \u{2190} shown below"
                } else {
                    ""
                };
                writeln!(
                    f,
                    "  {:<10}  {}   {status}{marker}",
                    platforms_label(&c.platforms),
                    c.path.display()
                )?;
            }
            writeln!(f)?;
            if let Some(focus) = self.copies.iter().find(|c| c.is_focus) {
                writeln!(
                    f,
                    "Showing the {} copy ({}{edited})  (\u{2212} {}, + installed):",
                    platforms_label(&focus.platforms),
                    self.installed_version.as_deref().unwrap_or("unversioned"),
                    self.source_name
                )?;
            }
        } else {
            let installed_ver = self.installed_version.as_deref().unwrap_or("unversioned");
            let source_ver = self.source_version.as_deref().unwrap_or("unversioned");
            writeln!(
                f,
                "  installed  {}  ({installed_ver}{edited})",
                self.installed_path.display()
            )?;
            writeln!(f, "  {}  {}  ({source_ver})", self.source_name, self.source_path.display())?;
        }
        writeln!(f)
    }
}

#[cfg(feature = "llm")]
impl fmt::Display for DiffOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_up_to_date {
            return if self.copies.len() > 1 {
                writeln!(
                    f,
                    "{} matches {} on all {} installed copies.",
                    self.artifact_name,
                    self.source_name,
                    self.copies.len()
                )
            } else {
                writeln!(f, "{} matches {}.", self.artifact_name, self.source_name)
            };
        }

        writeln!(f, "Comparing {} ({}) vs {}", self.artifact_name, self.kind, self.source_name)?;
        writeln!(f)?;
        self.write_header(f)?;

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

        // The full line-by-line diff is the firehose — only on request. The
        // header, file summary, and analysis already convey the shape; `--full`
        // is there when the exact lines matter.
        let has_diff = self.diff_text.as_ref().is_some_and(|d| !d.is_empty());
        if self.show_full {
            if let Some(diff) = &self.diff_text {
                if !diff.is_empty() {
                    writeln!(f, "Diff  (\u{2212} {}, + installed):", self.source_name)?;
                    write!(f, "{diff}")?;
                    writeln!(f)?;
                }
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

        if has_diff && !self.show_full {
            writeln!(f, "\n(run with --full to see the line-by-line diff)")?;
        }

        Ok(())
    }
}

#[cfg(all(test, feature = "llm"))]
mod tests {
    use crate::diff::{CopyStatus, DiffOutput, FileChange, FileStatus, Reconciliation};
    use crate::platform::Platform;
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
            show_full: false,
            copies: vec![CopyStatus {
                platforms: vec![Platform::Claude],
                path: PathBuf::from("/u/.claude/skills/personal-finance"),
                matches: false,
                added: 134,
                removed: 6,
                is_focus: true,
            }],
        }
    }

    /// A skill whose Claude copy matches the home but whose Codex copy differs —
    /// the case that used to read as a misleading "matches home".
    fn diverged_across_platforms() -> DiffOutput {
        let mut d = diverged();
        d.installed_path = PathBuf::from("/u/.agents/skills/personal-finance");
        d.reconciliations = vec![Reconciliation {
            description: "keep the installed edits, update the home".to_string(),
            command: "cmx skill promote personal-finance --platform codex".to_string(),
            note: None,
        }];
        d.copies = vec![
            CopyStatus {
                platforms: vec![Platform::Claude],
                path: PathBuf::from("/u/.claude/skills/personal-finance"),
                matches: true,
                added: 0,
                removed: 0,
                is_focus: false,
            },
            CopyStatus {
                platforms: vec![Platform::Codex],
                path: PathBuf::from("/u/.agents/skills/personal-finance"),
                matches: false,
                added: 11,
                removed: 2,
                is_focus: true,
            },
        ];
        d
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

    #[test]
    fn compact_default_hides_raw_diff_and_hints_at_full() {
        let out = diverged().to_string();
        assert!(!out.contains("Diff  (\u{2212} home"), "raw diff hidden by default: {out}");
        assert!(!out.contains("+ new"), "raw diff lines hidden: {out}");
        assert!(out.contains("run with --full"), "hints at --full: {out}");
        // The digestible parts still show.
        assert!(out.contains("Changed files"), "summary table kept: {out}");
        assert!(out.contains("Summary:"), "analysis kept: {out}");
    }

    #[test]
    fn full_shows_raw_diff_and_drops_the_hint() {
        let mut r = diverged();
        r.show_full = true;
        let out = r.to_string();
        assert!(out.contains("Diff  (\u{2212} home, + installed):"), "raw diff shown: {out}");
        assert!(out.contains("+ new"), "raw diff lines shown: {out}");
        assert!(!out.contains("run with --full"), "no hint when already full: {out}");
    }

    #[test]
    fn multi_platform_matrix_shows_per_copy_status() {
        let out = diverged_across_platforms().to_string();
        // The matrix names both copies and their status — no false "matches".
        assert!(out.contains("matches home"), "claude copy shown as matching: {out}");
        assert!(out.contains("differs  (+11"), "codex copy shown as differing: {out}");
        assert!(out.contains("Showing the codex copy"), "focuses the differing copy: {out}");
        assert!(out.contains("\u{2190} shown below"), "marks the focused copy: {out}");
        // Reconcile targets the focused (codex) copy.
        assert!(
            out.contains("cmx skill promote personal-finance --platform codex"),
            "reconcile qualified to codex: {out}"
        );
    }

    #[test]
    fn multi_platform_up_to_date_says_all_copies() {
        let mut r = diverged_across_platforms();
        r.is_up_to_date = true;
        for c in &mut r.copies {
            c.matches = true;
            c.is_focus = false;
        }
        let out = r.to_string();
        assert!(out.contains("matches home on all 2 installed copies"), "got: {out}");
    }
}
