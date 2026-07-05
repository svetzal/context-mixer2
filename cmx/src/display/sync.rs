use std::fmt;

use crate::diff::{FileChange, FileStatus};
use crate::platform::platforms_label;
use crate::sync::SyncResult;

fn version_label(version: Option<&str>) -> &str {
    version.unwrap_or("unversioned")
}

fn change_counts(changes: &[FileChange]) -> (usize, usize, usize) {
    changes
        .iter()
        .fold((0, 0, 0), |(modified, added, deleted), change| match change.status {
            FileStatus::Modified => (modified + 1, added, deleted),
            FileStatus::OnlyInInstalled => (modified, added, deleted + 1),
            FileStatus::OnlyInSource => (modified, added + 1, deleted),
        })
}

impl fmt::Display for SyncResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.external {
            writeln!(
                f,
                "Note: '{}' is external (managed by another tool); reconciling its copies anyway.",
                self.name
            )?;
        }

        if self.already_synced {
            return writeln!(f, "'{}' is already in sync across its install locations.", self.name);
        }

        let winner = platforms_label(&self.winner_platforms);
        let winner_v = version_label(self.winner_version.as_deref());
        if !self.apply {
            writeln!(f, "Plan to reconcile '{}' from {winner} ({winner_v}):", self.name)?;
            writeln!(f, "  source: {}", self.winner_path.display())?;
            for target in &self.targets {
                let (modified, added, deleted) = change_counts(&target.file_changes);
                writeln!(
                    f,
                    "  {} -> {}  [{}]  ({} -> {})",
                    self.winner_path.display(),
                    target.artifact_path.display(),
                    platforms_label(&target.platforms),
                    version_label(target.from_version.as_deref()),
                    winner_v,
                )?;
                writeln!(f, "    files: {modified} modified, {added} added, {deleted} deleted")?;
                for change in &target.file_changes {
                    let detail = match change.status {
                        FileStatus::Modified => {
                            format!("modified (+{} -{})", change.added, change.removed)
                        }
                        FileStatus::OnlyInInstalled => format!("deleted (-{})", change.added),
                        FileStatus::OnlyInSource => format!("added (+{})", change.removed),
                    };
                    writeln!(
                        f,
                        "    {}  {detail}",
                        target.artifact_path.join(&change.path).display()
                    )?;
                }
            }
            return writeln!(f, "Re-run with --apply to make these changes.");
        }

        writeln!(
            f,
            "Reconciled '{}' from {winner} ({winner_v}); {} target{} changed.",
            self.name,
            self.targets.len(),
            if self.targets.len() == 1 { "" } else { "s" }
        )?;
        writeln!(f, "  source: {}", self.winner_path.display())?;
        for target in &self.targets {
            writeln!(
                f,
                "  updated {} [{}]",
                target.artifact_path.display(),
                platforms_label(&target.platforms),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::diff::{FileChange, FileStatus};
    use crate::platform::Platform;
    use crate::sync::{SyncResult, SyncTarget};
    use std::path::PathBuf;

    fn base_result() -> SyncResult {
        SyncResult {
            name: "my-skill".to_string(),
            apply: false,
            external: false,
            winner_platforms: vec![Platform::Claude],
            winner_path: PathBuf::from("/claude/my-skill"),
            winner_version: Some("2.0.0".to_string()),
            already_synced: false,
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/copilot"),
                artifact_path: PathBuf::from("/copilot/my-skill"),
                from_version: Some("1.0.0".to_string()),
                file_changes: vec![
                    FileChange {
                        path: "SKILL.md".to_string(),
                        status: FileStatus::Modified,
                        added: 1,
                        removed: 1,
                    },
                    FileChange {
                        path: "extra.md".to_string(),
                        status: FileStatus::OnlyInInstalled,
                        added: 2,
                        removed: 0,
                    },
                    FileChange {
                        path: "new.md".to_string(),
                        status: FileStatus::OnlyInSource,
                        added: 0,
                        removed: 3,
                    },
                ],
            }],
        }
    }

    #[test]
    fn already_synced_branch() {
        let r = SyncResult {
            already_synced: true,
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("is already in sync across its install locations"), "got: {out}");
    }

    #[test]
    fn external_note_branch() {
        let out = SyncResult {
            external: true,
            ..base_result()
        }
        .to_string();
        assert!(out.contains("is external (managed by another tool)"), "got: {out}");
    }

    #[test]
    fn plan_mode_ends_with_apply_hint() {
        let out = base_result().to_string();
        assert!(out.contains("Plan to reconcile"), "got: {out}");
        assert!(out.contains("/copilot/my-skill/SKILL.md"), "lists changed file: {out}");
        assert!(out.contains("files: 1 modified, 1 added, 1 deleted"), "got: {out}");
        assert!(out.contains("/copilot/my-skill/extra.md  deleted (-2)"), "got: {out}");
        assert!(out.contains("/copilot/my-skill/new.md  added (+3)"), "got: {out}");
        assert!(out.trim_end().ends_with("Re-run with --apply to make these changes."));
    }

    #[test]
    fn apply_mode_reports_changed_targets() {
        let out = SyncResult {
            apply: true,
            ..base_result()
        }
        .to_string();
        assert!(out.contains("Reconciled 'my-skill' from claude (2.0.0); 1 target changed."));
        assert!(out.contains("updated /copilot/my-skill [copilot]"), "got: {out}");
    }
}
