use std::fmt;

use crate::sync::SyncResult;

fn platforms_label(platforms: &[crate::platform::Platform]) -> String {
    platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

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
