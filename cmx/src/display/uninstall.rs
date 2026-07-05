use std::fmt;

use crate::platform::platforms_label;
use crate::uninstall::{BatchUninstallResult, UninstallResult};

use super::util;

impl fmt::Display for UninstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tools = platforms_label(&self.platforms);
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

impl fmt::Display for BatchUninstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        util::write_each(f, &self.removed)?;
        for (name, hint) in &self.not_found {
            writeln!(f, "Not found (nothing to uninstall): {name}. {hint}")?;
        }
        if self.removed.is_empty() && self.not_found.is_empty() {
            writeln!(f, "No {}s given to uninstall.", self.kind)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ArtifactKind;

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

    #[test]
    fn batch_uninstall_result_includes_hint_for_near_miss() {
        let out = BatchUninstallResult {
            kind: ArtifactKind::Skill,
            removed: vec![],
            not_found: vec![("focus-skll".to_string(), "Did you mean 'focus-skill'?".to_string())],
        }
        .to_string();
        assert!(out.contains("focus-skll"), "{out}");
        assert!(out.contains("Did you mean 'focus-skill'?"), "{out}");
    }
}
