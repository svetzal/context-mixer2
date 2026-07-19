use std::fmt;

use crate::platform::platforms_label;
use crate::promote::PromoteResult;

use super::util::{change_counts, version_label, write_change_lines};

impl fmt::Display for PromoteResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.already_current {
            return writeln!(f, "'{}' already matches the home — nothing to promote.", self.name);
        }

        let source_platforms = platforms_label(&self.source_platforms);
        let version = version_label(self.version.as_deref());
        let (modified, added, deleted) = change_counts(&self.file_changes);

        if !self.apply {
            writeln!(f, "Plan to promote '{}' from {source_platforms} ({version}):", self.name)?;
            writeln!(f, "  source: {}", self.source_path.display())?;
            writeln!(f, "  target: {}", self.home_path.display())?;
            writeln!(f, "  files: {modified} modified, {added} added, {deleted} deleted")?;
            write_change_lines(f, &self.home_path, &self.file_changes)?;
            if !self.retracked.is_empty() {
                writeln!(f, "  re-track: {}", platforms_label(&self.retracked))?;
            }
            if !self.still_divergent.is_empty() {
                writeln!(
                    f,
                    "  still divergent after promote: {}",
                    platforms_label(&self.still_divergent)
                )?;
            }
            return writeln!(f, "Re-run with --apply to make these changes.");
        }

        let changed_files = self.file_changes.len();
        writeln!(
            f,
            "Promoted {source_platforms} copy of '{}' into home; {changed_files} file{} changed.",
            self.name,
            if changed_files == 1 { "" } else { "s" }
        )?;
        writeln!(f, "  source: {}", self.source_path.display())?;
        writeln!(f, "  target: {}", self.home_path.display())?;
        if !self.retracked.is_empty() {
            writeln!(f, "  re-tracked for: {}", platforms_label(&self.retracked))?;
        }
        if !self.still_divergent.is_empty() {
            writeln!(
                f,
                "  note: {} still differ(s) from the promoted copy and now read(s) as drifted — \
                 reconcile with `cmx {} sync {}` or promote from there.",
                platforms_label(&self.still_divergent),
                self.kind,
                self.name,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::diff::{FileChange, FileStatus};
    use crate::platform::Platform;
    use crate::promote::PromoteResult;
    use crate::types::ArtifactKind;
    use std::path::PathBuf;

    fn base_result() -> PromoteResult {
        PromoteResult {
            name: "personal-finance".to_string(),
            kind: ArtifactKind::Skill,
            source_path: PathBuf::from("/Users/me/.claude/skills/personal-finance"),
            source_platforms: vec![Platform::Claude],
            home_path: PathBuf::from("/home/skills/personal-finance"),
            apply: false,
            already_current: false,
            version: Some("1.2.0".to_string()),
            file_changes: vec![
                FileChange {
                    path: "SKILL.md".to_string(),
                    status: FileStatus::Modified,
                    added: 1,
                    removed: 1,
                },
                FileChange {
                    path: "obsolete.md".to_string(),
                    status: FileStatus::OnlyInInstalled,
                    added: 2,
                    removed: 0,
                },
                FileChange {
                    path: "fresh.md".to_string(),
                    status: FileStatus::OnlyInSource,
                    added: 0,
                    removed: 4,
                },
            ],
            retracked: vec![Platform::Claude, Platform::Codex],
            still_divergent: vec![],
        }
    }

    #[test]
    fn promote_already_current_message() {
        let r = PromoteResult {
            already_current: true,
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("already matches the home"), "got: {out}");
    }

    #[test]
    fn promote_plan_ends_with_apply_hint_and_lists_files() {
        let out = base_result().to_string();
        assert!(out.contains("Plan to promote"), "got: {out}");
        assert!(out.contains("/home/skills/personal-finance/SKILL.md"), "lists file path: {out}");
        assert!(out.contains("files: 1 modified, 1 added, 1 deleted"), "got: {out}");
        assert!(
            out.contains("/home/skills/personal-finance/obsolete.md  deleted (-2)"),
            "got: {out}"
        );
        assert!(out.contains("/home/skills/personal-finance/fresh.md  added (+4)"), "got: {out}");
        assert!(out.trim_end().ends_with("Re-run with --apply to make these changes."));
    }

    #[test]
    fn promote_apply_reports_countable_change() {
        let out = PromoteResult {
            apply: true,
            ..base_result()
        }
        .to_string();
        assert!(
            out.contains("Promoted claude copy of 'personal-finance' into home; 3 files changed.")
        );
        assert!(out.contains("claude, codex"), "lists re-tracked platforms: {out}");
    }

    #[test]
    fn promote_warns_about_still_divergent_platforms() {
        let out = PromoteResult {
            apply: true,
            still_divergent: vec![Platform::Codex],
            ..base_result()
        }
        .to_string();
        assert!(out.contains("still differ"), "warns about divergence: {out}");
        assert!(out.contains("cmx skill sync personal-finance"), "points at sync: {out}");
    }
}
