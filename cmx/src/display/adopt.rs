//! Output formatting for `cmx adopt`, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::adopt::{AdoptOutcome, UnadoptOutcome};
use crate::platform::platforms_label;

impl fmt::Display for AdoptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.adopted.is_empty() {
            return writeln!(
                f,
                "Nothing to adopt — no orphaned artifacts found{}.",
                if self.included_local {
                    " (global + project scope)"
                } else {
                    ""
                }
            );
        }
        writeln!(f, "Adopted {} artifact(s) into {}:", self.adopted.len(), self.home.display())?;
        for a in &self.adopted {
            writeln!(
                f,
                "  {} {} — now tracked for: {}",
                a.kind,
                a.name,
                platforms_label(&a.platforms)
            )?;
        }
        writeln!(f)?;
        writeln!(
            f,
            "Project them to another tool with: cmx skill install --all --platform <tool>"
        )
    }
}

impl fmt::Display for UnadoptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.unadopted {
            write!(f, "Unadopted {} ({}) — removed from the home", r.name, r.kind)?;
            if r.untracked_from.is_empty() {
                writeln!(f, ".")?;
            } else {
                writeln!(f, "; un-tracked for: {}.", platforms_label(&r.untracked_from))?;
            }
        }
        if !self.not_adopted.is_empty() {
            writeln!(f, "Not adopted (nothing in the home): {}", self.not_adopted.join(", "))?;
        }
        if !self.unadopted.is_empty() {
            writeln!(
                f,
                "\nThe on-disk copies remain (now orphaned). Mark them external with \
                 `cmx config external add <name>` if another tool manages them."
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::types::ArtifactKind;
    use std::path::PathBuf;

    // --- AdoptOutcome ---

    #[test]
    fn adopt_outcome_empty_message() {
        let o = AdoptOutcome {
            adopted: vec![],
            home: PathBuf::from("/home/u/.config/context-mixer/home"),
            included_local: false,
        };
        assert!(o.to_string().contains("Nothing to adopt"));
    }

    #[test]
    fn adopt_outcome_lists_adopted_and_projection_hint() {
        let o = AdoptOutcome {
            adopted: vec![crate::adopt::AdoptResult {
                kind: ArtifactKind::Skill,
                name: "my-skill".to_string(),
                home_path: PathBuf::from("/home/u/.config/context-mixer/home/skills/my-skill"),
                platforms: vec![Platform::Claude],
            }],
            home: PathBuf::from("/home/u/.config/context-mixer/home"),
            included_local: false,
        };
        let out = o.to_string();
        assert!(out.contains("Adopted 1 artifact(s)"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("now tracked for: claude"));
        assert!(out.contains("install --all --platform"), "projection hint present");
    }
}
