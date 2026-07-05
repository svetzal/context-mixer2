use std::fmt;

use crate::sets::{
    SetAddResult, SetCreateResult, SetDeleteResult, SetListResult, SetRemoveResult,
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
    use crate::sets::{SetListEntry, SetMemberStatus};
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
