use std::fmt;

use crate::sets::{
    MemberActivateOutcome, MemberDeactivateOutcome, SetActivateResult, SetAddResult,
    SetCreateResult, SetDeactivateResult, SetDeleteResult, SetListResult, SetRemoveResult,
    SetRenameResult, SetShowResult,
};
use crate::table::render_table;

impl fmt::Display for SetCreateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Set '{}' created.", self.name)
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
                vec![
                    e.name.clone(),
                    e.state.to_string(),
                    e.member_count.to_string(),
                ]
            })
            .collect();
        write!(f, "{}", render_table(vec!["Name", "State", "Members"], 3, rows))
    }
}

impl fmt::Display for SetShowResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Set '{}' ({})", self.name, self.state)?;
        if let Some(desc) = &self.description {
            writeln!(f, "  {desc}")?;
        }
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
            writeln!(f, "  {} {} (source: {source}) [{status}]", member.kind, member.name)?;
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
        if self.dry_run {
            writeln!(f, "Would activate set '{}':", self.name)?;
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
                MemberActivateOutcome::Installed if self.dry_run => "would install".to_string(),
                MemberActivateOutcome::Installed => "installed".to_string(),
                MemberActivateOutcome::AlreadyInstalled => "already installed".to_string(),
                MemberActivateOutcome::Unresolvable(reason) => format!("unresolvable ({reason})"),
                MemberActivateOutcome::Failed(reason) => format!("failed ({reason})"),
            };
            writeln!(f, "  {} {}: {line}", m.kind, m.name)?;
        }
        Ok(())
    }
}

impl fmt::Display for SetDeactivateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dry_run {
            writeln!(f, "Would deactivate set '{}':", self.name)?;
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
            let line = match &m.outcome {
                MemberDeactivateOutcome::Uninstalled if self.dry_run => {
                    "would uninstall".to_string()
                }
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
        }
        Ok(())
    }
}

impl fmt::Display for SetDeleteResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Set '{}' deleted.", self.name)
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
        };
        assert_eq!(r.to_string(), "Set 'rust-work' created.\n");
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
            }],
        };
        let out = r.to_string();
        assert!(out.contains("rust-work"));
        assert!(out.contains("active"));
        assert!(out.contains('2'));
        assert!(!out.contains("Footprint"), "Phase 3 column must not appear yet");
    }

    #[test]
    fn set_show_result_no_members() {
        let r = SetShowResult {
            name: "blog".to_string(),
            description: None,
            state: SetState::Inactive,
            members: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("blog"));
        assert!(out.contains("(no members)"));
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
                },
                SetMemberStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    source: None,
                    installed: false,
                },
            ],
        };
        let out = r.to_string();
        assert!(out.contains("Rust craftsmanship"));
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("source: guidelines"));
        assert!(out.contains("[installed]"));
        assert!(out.contains("foundry"));
        assert!(out.contains("[not installed]"));
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
                },
                MemberActivateStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    outcome: MemberActivateOutcome::AlreadyInstalled,
                },
            ],
            any_failed: false,
            dry_run: false,
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
            }],
            any_failed: true,
            dry_run: false,
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
            }],
            any_failed: false,
            dry_run: true,
        };
        let out = r.to_string();
        assert!(out.contains("Would activate set 'rust-work'"));
        assert!(out.contains("rust-craftsperson: would install"));
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
                },
                MemberDeactivateStatus {
                    kind: ArtifactKind::Skill,
                    name: "foundry".to_string(),
                    outcome: MemberDeactivateOutcome::Retained("blog".to_string()),
                },
            ],
            any_blocked: false,
            dry_run: false,
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
            }],
            any_blocked: true,
            dry_run: false,
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
            }],
            any_blocked: false,
            dry_run: true,
        };
        let out = r.to_string();
        assert!(out.contains("Would deactivate set 'rust-work'"));
        assert!(out.contains("rust-craftsperson: would uninstall"));
    }

    #[test]
    fn set_delete_result_display() {
        let r = SetDeleteResult {
            name: "blog".to_string(),
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
