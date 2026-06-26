use std::fmt;

use crate::platform::platforms_label;
use crate::sync::SyncResult;

fn version_label(version: Option<&str>) -> String {
    version.map_or_else(|| "unversioned".to_string(), |v| format!("v{v}"))
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
        let verb = if self.dry_run {
            "Would reconcile"
        } else {
            "Reconciled"
        };
        writeln!(f, "{verb} '{}' from {winner} ({winner_v}):", self.name)?;
        for t in &self.targets {
            let arrow = if self.dry_run {
                "would update"
            } else {
                "updated"
            };
            writeln!(
                f,
                "  {arrow} {} ({} → {winner_v})  [{}]",
                t.location.display(),
                version_label(t.from_version.as_deref()),
                platforms_label(&t.platforms),
            )?;
        }
        if self.dry_run {
            writeln!(f, "(dry run — nothing was written)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::Platform;
    use crate::sync::{SyncResult, SyncTarget};
    use std::path::PathBuf;

    fn base_result() -> SyncResult {
        SyncResult {
            name: "my-skill".to_string(),
            dry_run: false,
            external: false,
            winner_platforms: vec![Platform::Claude],
            winner_version: None,
            already_synced: false,
            targets: Vec::new(),
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
        assert!(!out.contains("Reconciled"), "got: {out}");
        assert!(!out.contains("Would reconcile"), "got: {out}");
    }

    #[test]
    fn external_note_branch() {
        let r = SyncResult {
            external: true,
            already_synced: false,
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/some/dir"),
                from_version: None,
            }],
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("is external (managed by another tool)"), "got: {out}");
    }

    #[test]
    fn live_reconcile_path() {
        let r = SyncResult {
            dry_run: false,
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/copilot/dir"),
                from_version: Some("1.0.0".to_string()),
            }],
            winner_version: Some("2.0.0".to_string()),
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("Reconciled 'my-skill' from"), "got: {out}");
        assert!(out.contains("updated"), "got: {out}");
        assert!(!out.contains("dry run"), "got: {out}");
    }

    #[test]
    fn dry_run_path() {
        let r = SyncResult {
            dry_run: true,
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/copilot/dir"),
                from_version: Some("1.0.0".to_string()),
            }],
            winner_version: Some("2.0.0".to_string()),
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("Would reconcile"), "got: {out}");
        assert!(out.contains("would update"), "got: {out}");
        assert!(out.contains("(dry run — nothing was written)"), "got: {out}");
    }

    #[test]
    fn version_label_some() {
        let r = SyncResult {
            winner_version: Some("1.2.0".to_string()),
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/dir"),
                from_version: None,
            }],
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("v1.2.0"), "got: {out}");
    }

    #[test]
    fn version_label_none() {
        let r = SyncResult {
            winner_version: None,
            targets: vec![SyncTarget {
                platforms: vec![Platform::Copilot],
                location: PathBuf::from("/dir"),
                from_version: None,
            }],
            ..base_result()
        };
        let out = r.to_string();
        assert!(out.contains("unversioned"), "got: {out}");
    }
}
