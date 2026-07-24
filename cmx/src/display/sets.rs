//! Output formatting for `cmx set` subcommands, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::sets::{
    MemberActivateOutcome, MemberDeactivateOutcome, SetActivateResult, SetAddResult,
    SetCreateResult, SetDeactivateResult, SetDeleteResult, SetListResult, SetRemoveResult,
    SetRenameResult, SetShowResult,
};
use crate::table::render_table;
use crate::types::SetState;

/// Render a character count as an approximate, human-scaled footprint (e.g.
/// `~2.1k chars`) — see `SETS.md`, "Context-footprint reporting". Ships as a
/// character count in Phase 3; a token estimate may follow later.
fn format_footprint(chars: usize) -> String {
    if chars >= 1000 {
        // Integer-only `chars / 1000.tenths` — avoids a usize→f64 precision
        // cast for what is, at most, one decimal digit of rounding.
        let whole = chars / 1000;
        let tenths = (chars % 1000) / 100;
        format!("~{whole}.{tenths}k chars")
    } else {
        format!("~{chars} chars")
    }
}

fn activation_targets_line(targets: &[crate::sets::MemberActivateTarget]) -> String {
    targets
        .iter()
        .map(|target| {
            format!("{} -> {}", target.source_path.display(), target.target_path.display())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn deactivate_targets_line(targets: &[crate::sets::MemberDeactivateTarget]) -> String {
    targets
        .iter()
        .map(|target| target.artifact_path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for SetCreateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.seeded_from {
            Some(spec) => writeln!(
                f,
                "Set '{}' created, seeded with {} member(s) from {spec}.",
                self.name, self.member_count
            ),
            None => writeln!(f, "Set '{}' created.", self.name),
        }
    }
}

impl fmt::Display for SetListResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entries.is_empty() {
            return write!(f, "No sets defined.\n\nCreate one with: cmx set create <name>\n");
        }
        let rows = self
            .entries
            .iter()
            .map(|e| {
                let mut footprint = format_footprint(e.footprint_chars);
                if e.state == SetState::Inactive {
                    footprint.push_str(" (not loaded)");
                }
                vec![
                    e.name.clone(),
                    e.state.to_string(),
                    e.member_count.to_string(),
                    footprint,
                ]
            })
            .collect();
        write!(f, "{}", render_table(vec!["Name", "State", "Members", "Footprint"], 3, rows))
    }
}

impl fmt::Display for SetShowResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Set '{}' ({})", self.name, self.state)?;
        if let Some(desc) = &self.description {
            writeln!(f, "  {desc}")?;
        }
        let mut footprint = format_footprint(self.footprint_chars);
        if self.state == SetState::Inactive {
            footprint.push_str(" (not loaded)");
        }
        writeln!(f, "  Footprint: {footprint}")?;
        if self.members.is_empty() {
            writeln!(f, "  (no members)")?;
            return Ok(());
        }
        for member in &self.members {
            let source = member.source.as_deref().unwrap_or("-");
            let status = if member.installed {
                "installed"
            } else {
                "not installed"
            };
            let chars = member.footprint_chars.map_or_else(|| "?".to_string(), format_footprint);
            writeln!(f, "  {} {} (source: {source}) [{status}] {chars}", member.kind, member.name)?;
        }
        Ok(())
    }
}

impl fmt::Display for SetAddResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.added.is_empty() {
            writeln!(f, "Added to '{}': {}", self.set, self.added.join(", "))?;
        }
        if !self.already.is_empty() {
            writeln!(f, "Already in '{}': {}", self.set, self.already.join(", "))?;
        }
        if self.added.is_empty() && self.already.is_empty() {
            writeln!(f, "Nothing to add to '{}'.", self.set)?;
        }
        Ok(())
    }
}

impl fmt::Display for SetRemoveResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.removed.is_empty() {
            writeln!(f, "Removed from '{}': {}", self.set, self.removed.join(", "))?;
        }
        if !self.not_found.is_empty() {
            writeln!(f, "Not in '{}': {}", self.set, self.not_found.join(", "))?;
        }
        if self.removed.is_empty() && self.not_found.is_empty() {
            writeln!(f, "Nothing to remove from '{}'.", self.set)?;
        }
        Ok(())
    }
}

impl fmt::Display for SetActivateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.apply {
            writeln!(f, "Plan to activate set '{}':", self.name)?;
        } else if self.any_failed {
            writeln!(f, "Set '{}' partially activated:", self.name)?;
        } else {
            writeln!(f, "Set '{}' activated.", self.name)?;
        }
        if self.members.is_empty() {
            writeln!(f, "  (no members)")?;
            return Ok(());
        }
        for m in &self.members {
            let line = match &m.outcome {
                MemberActivateOutcome::Installed if !self.apply => "install".to_string(),
                MemberActivateOutcome::Installed => "installed".to_string(),
                MemberActivateOutcome::AlreadyInstalled => "already installed".to_string(),
                MemberActivateOutcome::Unresolvable(reason) => format!("unresolvable ({reason})"),
                MemberActivateOutcome::Failed(reason) => format!("failed ({reason})"),
            };
            writeln!(f, "  {} {}: {line}", m.kind, m.name)?;
            if !m.targets.is_empty() {
                writeln!(f, "    {}", activation_targets_line(&m.targets))?;
            }
        }
        if !self.apply {
            writeln!(f, "Re-run with --apply to make these changes.")?;
        }
        Ok(())
    }
}

impl fmt::Display for SetDeactivateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.apply {
            writeln!(f, "Plan to deactivate set '{}':", self.name)?;
        } else if self.any_blocked {
            writeln!(f, "Set '{}' partially deactivated:", self.name)?;
        } else {
            writeln!(f, "Set '{}' deactivated.", self.name)?;
        }
        if self.members.is_empty() {
            writeln!(f, "  (no members)")?;
            return Ok(());
        }
        for m in &self.members {
            let has_discarded_paths =
                m.targets.iter().any(|target| !target.discarded_paths.is_empty());
            let line = match &m.outcome {
                MemberDeactivateOutcome::Uninstalled if !self.apply && has_discarded_paths => {
                    "uninstall (--force will discard local edits)".to_string()
                }
                MemberDeactivateOutcome::Uninstalled if !self.apply => "uninstall".to_string(),
                MemberDeactivateOutcome::Uninstalled => "uninstalled".to_string(),
                MemberDeactivateOutcome::NotInstalled => "not installed".to_string(),
                MemberDeactivateOutcome::Retained(holder) => {
                    format!("retained (held by set '{holder}')")
                }
                MemberDeactivateOutcome::DriftBlocked => {
                    "blocked: local edits — pass --force to discard them".to_string()
                }
            };
            writeln!(f, "  {} {}: {line}", m.kind, m.name)?;
            if !m.targets.is_empty() {
                writeln!(f, "    {}", deactivate_targets_line(&m.targets))?;
            }
            if has_discarded_paths {
                for target in &m.targets {
                    for path in &target.discarded_paths {
                        writeln!(f, "    would discard {}", path.display())?;
                    }
                }
            }
        }
        if !self.apply {
            writeln!(f, "Re-run with --apply to make these changes.")?;
        }
        Ok(())
    }
}

impl fmt::Display for SetDeleteResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.purge {
            return writeln!(f, "Set '{}' deleted.", self.name);
        }
        if let Some(deactivate) = &self.deactivate {
            write!(f, "{deactivate}")?;
        }
        if self.deleted {
            writeln!(f, "Set '{}' deleted.", self.name)
        } else if !self.apply {
            writeln!(f, "Set '{}' would be deleted after the purge plan above.", self.name)
        } else {
            writeln!(
                f,
                "Set '{}' was not deleted because the purge was blocked by local edits.",
                self.name
            )
        }
    }
}

impl fmt::Display for SetRenameResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Set '{}' renamed to '{}'.", self.old, self.new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sets::{
        MemberActivateStatus, MemberDeactivateStatus, SetListEntry, SetMemberStatus,
    };
    use crate::types::{ArtifactKind, SetState};

    #[test]
    fn set_create_result_display() {
        let r = SetCreateResult {
            name: "rust-work".to_string(),
            member_count: 0,
            seeded_from: None,
        };
        assert_eq!(r.to_string(), "Set 'rust-work' created.\n");
    }

    #[test]
    fn set_create_result_display_seeded() {
        let r = SetCreateResult {
            name: "rust-work".to_string(),
            member_count: 2,
            seeded_from: Some("guidelines:my-plugin".to_string()),
        };
        assert_eq!(
            r.to_string(),
            "Set 'rust-work' created, seeded with 2 member(s) from guidelines:my-plugin.\n"
        );
    }

    #[test]
    fn set_list_result_empty_shows_hint() {
        let r = SetListResult { entries: vec![] };
        let out = r.to_string();
        assert!(out.contains("No sets defined."));
        assert!(out.contains("cmx set create"));
    }

    #[test]
    fn set_list_result_populated_shows_name_state_members() {
        let r = SetListResult {
            entries: vec![SetListEntry {
                name: "rust-work".to_string(),
                state: SetState::Active,
                member_count: 2,
                footprint_chars: 2100,
            }],
        };
        let out = r.to_string();
        assert!(out.contains("rust-work"));
        assert!(out.contains("active"));
        assert!(out.contains('2'));
        assert!(out.contains("Footprint"), "Phase 3 column present");
        assert!(
            out.contains("~2.1k chars"),
            "active set footprint shown without annotation: {out}"
        );
        assert!(!out.contains("not loaded"), "active set is currently loaded: {out}");
    }

    #[test]
    fn set_list_result_inactive_footprint_annotated_not_loaded() {
        let r = SetListResult {
            entries: vec![SetListEntry {
                name: "client-ort".to_string(),
                state: SetState::Inactive,
                member_count: 4,
                footprint_chars: 1400,
            }],
        };
        let out = r.to_string();
        assert!(out.contains("~1.4k chars (not loaded)"), "inactive footprint annotated: {out}");
    }

    #[test]
    fn set_show_result_no_members() {
        let r = SetShowResult {
            name: "blog".to_string(),
            description: None,
            state: SetState::Inactive,
            members: vec![],
            footprint_chars: 0,
        };
        let out = r.to_string();
        assert!(out.contains("blog"));
        assert!(out.contains("(no members)"));
        assert!(out.contains("Footprint: ~0 chars (not loaded)"));
    }

    #[test]
    fn set_show_result_with_description_and_members() {
        let r = SetShowResult {
            name: "rust-work".to_string(),
            description: Some("Rust craftsmanship".to_string()),
            state: SetState::Active,
            members: vec![
                SetMemberStatus {
                    kind: ArtifactKind::Agent,
                    name: "rust-craftsperson".to_string(),
                    source: Some("guidelines".to_string()),
                    installed: true,
                    footprint_chars: Some(1500),
                },
                SetMemberStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    source: None,
                    installed: false,
                    footprint_chars: None,
                },
            ],
            footprint_chars: 1500,
        };
        let out = r.to_string();
        assert!(out.contains("Rust craftsmanship"));
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("source: guidelines"));
        assert!(out.contains("[installed]"));
        assert!(out.contains("foundry"));
        assert!(out.contains("[not installed]"));
        assert!(out.contains("Footprint: ~1.5k chars"));
        assert!(out.contains("~1.5k chars"), "installed member's own count shown");
        assert!(out.contains('?'), "unresolvable member rendered as ?");
    }

    #[test]
    fn set_add_result_shows_added_and_already() {
        let r = SetAddResult {
            set: "rust-work".to_string(),
            added: vec!["foundry".to_string()],
            already: vec!["rust-craftsperson".to_string()],
        };
        let out = r.to_string();
        assert!(out.contains("Added to 'rust-work': foundry"));
        assert!(out.contains("Already in 'rust-work': rust-craftsperson"));
    }

    #[test]
    fn set_remove_result_shows_removed_and_not_found() {
        let r = SetRemoveResult {
            set: "rust-work".to_string(),
            removed: vec!["foundry".to_string()],
            not_found: vec!["ghost".to_string()],
        };
        let out = r.to_string();
        assert!(out.contains("Removed from 'rust-work': foundry"));
        assert!(out.contains("Not in 'rust-work': ghost"));
    }

    #[test]
    fn set_activate_result_shows_installed_and_already_installed() {
        let r = SetActivateResult {
            name: "rust-work".to_string(),
            members: vec![
                MemberActivateStatus {
                    kind: ArtifactKind::Agent,
                    name: "rust-craftsperson".to_string(),
                    outcome: MemberActivateOutcome::Installed,
                    targets: Vec::new(),
                },
                MemberActivateStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    outcome: MemberActivateOutcome::AlreadyInstalled,
                    targets: Vec::new(),
                },
            ],
            any_failed: false,
            apply: true,
        };
        let out = r.to_string();
        assert!(out.contains("Set 'rust-work' activated."));
        assert!(out.contains("rust-craftsperson: installed"));
        assert!(out.contains("foundry: already installed"));
    }

    #[test]
    fn set_activate_result_shows_unresolvable_and_partial_failure() {
        let r = SetActivateResult {
            name: "rust-work".to_string(),
            members: vec![MemberActivateStatus {
                kind: ArtifactKind::Skill,
                name: "ghost".to_string(),
                outcome: MemberActivateOutcome::Unresolvable(
                    "source 'gone' is not registered".to_string(),
                ),
                targets: Vec::new(),
            }],
            any_failed: true,
            apply: true,
        };
        let out = r.to_string();
        assert!(out.contains("partially activated"));
        assert!(out.contains("ghost: unresolvable (source 'gone' is not registered)"));
    }

    #[test]
    fn set_activate_result_dry_run_says_would_install() {
        let r = SetActivateResult {
            name: "rust-work".to_string(),
            members: vec![MemberActivateStatus {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                outcome: MemberActivateOutcome::Installed,
                targets: Vec::new(),
            }],
            any_failed: false,
            apply: false,
        };
        let out = r.to_string();
        assert!(out.contains("Plan to activate set 'rust-work'"));
        assert!(out.contains("rust-craftsperson: install"));
        assert!(out.contains("Re-run with --apply"));
    }

    #[test]
    fn set_deactivate_result_shows_uninstalled_and_retained() {
        let r = SetDeactivateResult {
            name: "rust-work".to_string(),
            members: vec![
                MemberDeactivateStatus {
                    kind: ArtifactKind::Agent,
                    name: "rust-craftsperson".to_string(),
                    outcome: MemberDeactivateOutcome::Uninstalled,
                    targets: Vec::new(),
                },
                MemberDeactivateStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    outcome: MemberDeactivateOutcome::Retained("blog".to_string()),
                    targets: Vec::new(),
                },
            ],
            any_blocked: false,
            apply: true,
        };
        let out = r.to_string();
        assert!(out.contains("Set 'rust-work' deactivated."));
        assert!(out.contains("rust-craftsperson: uninstalled"));
        assert!(out.contains("foundry: retained (held by set 'blog')"));
    }

    #[test]
    fn set_deactivate_result_shows_drift_blocked_and_partial() {
        let r = SetDeactivateResult {
            name: "rust-work".to_string(),
            members: vec![MemberDeactivateStatus {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                outcome: MemberDeactivateOutcome::DriftBlocked,
                targets: Vec::new(),
            }],
            any_blocked: true,
            apply: true,
        };
        let out = r.to_string();
        assert!(out.contains("partially deactivated"));
        assert!(out.contains("blocked: local edits"));
        assert!(out.contains("--force"));
    }

    #[test]
    fn set_deactivate_result_dry_run_says_would_uninstall() {
        let r = SetDeactivateResult {
            name: "rust-work".to_string(),
            members: vec![MemberDeactivateStatus {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                outcome: MemberDeactivateOutcome::Uninstalled,
                targets: Vec::new(),
            }],
            any_blocked: false,
            apply: false,
        };
        let out = r.to_string();
        assert!(out.contains("Plan to deactivate set 'rust-work'"));
        assert!(out.contains("rust-craftsperson: uninstall"));
        assert!(out.contains("Re-run with --apply"));
    }

    #[test]
    fn set_delete_result_display() {
        let r = SetDeleteResult {
            name: "blog".to_string(),
            purge: false,
            apply: true,
            deleted: true,
            deactivate: None,
        };
        assert_eq!(r.to_string(), "Set 'blog' deleted.\n");
    }

    #[test]
    fn set_rename_result_display() {
        let r = SetRenameResult {
            old: "old".to_string(),
            new: "new".to_string(),
        };
        assert_eq!(r.to_string(), "Set 'old' renamed to 'new'.\n");
    }
}
