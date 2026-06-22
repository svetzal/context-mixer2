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
            ", edited locally"
        } else {
            ""
        };
        let changed = &self.changed_label;
        let changed_ver = self.installed_version.as_deref().unwrap_or("unversioned");
        let source_ver = self.source_version.as_deref().unwrap_or("unversioned");
        if self.copies.len() > 1 {
            for c in &self.copies {
                let status = if c.matches {
                    format!("matches {}", self.source_name)
                } else {
                    format!(
                        "differs from {} (+{} \u{2212}{})",
                        self.source_name, c.added, c.removed
                    )
                };
                let marker = if c.is_focus {
                    "   \u{2190} detailed below"
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
            writeln!(
                f,
                "Showing {changed} ({changed_ver}{edited}) against {} \u{2014} \
                 \u{2212} lines are {}, + lines are {changed}:",
                self.source_name, self.source_name
            )?;
        } else {
            writeln!(
                f,
                "  {changed:<8} {}  ({changed_ver}{edited})",
                self.installed_path.display()
            )?;
            writeln!(
                f,
                "  {:<8} {}  ({source_ver})",
                self.source_name,
                self.source_path.display()
            )?;
            writeln!(f)?;
            writeln!(f, "\u{2212} lines are {}, + lines are {changed}:", self.source_name)?;
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
            writeln!(f, "Changed files:")?;
            for c in &self.file_changes {
                let (flag, detail) = match c.status {
                    FileStatus::Modified => ('M', format!("+{}  \u{2212}{}", c.added, c.removed)),
                    FileStatus::OnlyInInstalled => ('A', format!("only in {}", self.changed_label)),
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
                    writeln!(
                        f,
                        "Line-by-line  (\u{2212} {}, + {}):",
                        self.source_name, self.changed_label
                    )?;
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
            changed_label: "claude".to_string(),
        }
    }

    /// A skill whose Claude copy matches the home but whose Codex copy differs —
    /// the case that used to read as a misleading "matches home".
    fn diverged_across_platforms() -> DiffOutput {
        let mut d = diverged();
        d.installed_path = PathBuf::from("/u/.agents/skills/personal-finance");
        d.changed_label = "codex".to_string();
        d.reconciliations = vec![Reconciliation {
            description: "keep codex's edits — copy codex into home".to_string(),
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
        // The changed side is named by its concrete platform (claude), not "installed".
        assert!(out.contains("claude"), "names the platform copy: {out}");
        assert!(out.contains("/u/.claude/skills/personal-finance"), "shows its path: {out}");
        assert!(!out.contains("installed  /u"), "drops the abstract 'installed' label: {out}");
        assert!(out.contains("edited locally"), "flags the local edit: {out}");
        assert!(out.contains("/u/.config/cmx/home/skills/personal-finance"), "{out}");
    }

    #[test]
    fn file_summary_uses_concrete_side_names() {
        let out = diverged().to_string();
        assert!(out.contains("Changed files"), "{out}");
        // Convention stated once, with concrete names, in the header.
        assert!(
            out.contains("\u{2212} lines are home, + lines are claude"),
            "states the convention concretely: {out}"
        );
        assert!(!out.contains("+ installed"), "no abstract 'installed': {out}");
        assert!(out.contains("M  SKILL.md"), "modified flag: {out}");
        assert!(out.contains("+124"), "added count: {out}");
        assert!(out.contains("A  references/new.md"), "added-file flag: {out}");
        assert!(out.contains("only in claude"), "names the platform copy: {out}");
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
        assert!(!out.contains("Line-by-line"), "raw diff hidden by default: {out}");
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
        assert!(
            out.contains("Line-by-line  (\u{2212} home, + claude):"),
            "raw diff shown with concrete labels: {out}"
        );
        assert!(out.contains("+ new"), "raw diff lines shown: {out}");
        assert!(!out.contains("run with --full"), "no hint when already full: {out}");
    }

    #[test]
    fn multi_platform_matrix_shows_per_copy_status() {
        let out = diverged_across_platforms().to_string();
        // The matrix names both copies and their status — no false "matches".
        assert!(out.contains("matches home"), "claude copy shown as matching: {out}");
        assert!(out.contains("differs from home (+11"), "codex copy shown as differing: {out}");
        assert!(out.contains("Showing codex"), "focuses the differing copy: {out}");
        assert!(out.contains("\u{2190} detailed below"), "marks the focused copy: {out}");
        assert!(out.contains("+ lines are codex"), "convention names codex: {out}");
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
