use std::fmt;

use crate::list::{ListKindOutput, ListOutput, section_str, table_str};
use crate::table::empty_state;
use crate::types::InstallScope;

impl fmt::Display for ListKindOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.kind;
        let empty = vec![];
        let global = self.rows.get(&InstallScope::Global).unwrap_or(&empty);
        let local = self.rows.get(&InstallScope::Local).unwrap_or(&empty);

        if global.is_empty() && local.is_empty() {
            return writeln!(f, "No {kind}s installed.");
        }

        if !global.is_empty() {
            writeln!(f, "Global {kind}s:")?;
            write!(f, "{}", table_str(global))?;
        }

        if !local.is_empty() {
            if !global.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Local {kind}s:")?;
            write!(f, "{}", table_str(local))?;
        }

        Ok(())
    }
}

impl fmt::Display for ListOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let empty = vec![];
        let global_agents = self.agents.get(&InstallScope::Global).unwrap_or(&empty);
        let local_agents = self.agents.get(&InstallScope::Local).unwrap_or(&empty);
        let global_skills = self.skills.get(&InstallScope::Global).unwrap_or(&empty);
        let local_skills = self.skills.get(&InstallScope::Local).unwrap_or(&empty);

        if global_agents.is_empty()
            && local_agents.is_empty()
            && global_skills.is_empty()
            && local_skills.is_empty()
        {
            return write!(f, "{}", empty_state("Nothing installed."));
        }

        write!(f, "{}", section_str("Global agents", global_agents))?;
        write!(f, "{}", section_str("Local agents", local_agents))?;
        write!(f, "{}", section_str("Global skills", global_skills))?;
        write!(f, "{}", section_str("Local skills", local_skills))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list::{ListStatus, Row};
    use crate::types::ArtifactKind;
    use std::collections::BTreeMap;

    fn make_row(name: &str) -> Row {
        Row {
            name: name.to_string(),
            installed_version: Some("1.0.0".to_string()),
            available_version: Some("1.0.0".to_string()),
            source: Some("src".to_string()),
            platforms: vec!["claude".to_string()],
            status: ListStatus::Ok,
        }
    }

    // --- Step 7: ListKindOutput and ListOutput ---

    #[test]
    fn list_kind_output_empty_shows_none_installed() {
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows: BTreeMap::new(),
        };
        assert_eq!(r.to_string(), "No agents installed.\n");
    }

    #[test]
    fn list_kind_output_global_only_shows_global_header() {
        let mut rows = BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("agent-a")]);
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows,
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("agent-a"));
        assert!(out.contains("Platforms"));
        assert!(out.contains("Status"));
    }

    #[test]
    fn list_kind_output_both_scopes_shows_both_headers() {
        let mut rows = BTreeMap::new();
        rows.insert(InstallScope::Global, vec![make_row("agent-g")]);
        rows.insert(InstallScope::Local, vec![make_row("agent-l")]);
        let r = ListKindOutput {
            kind: ArtifactKind::Agent,
            rows,
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("Local agents:"));
    }

    #[test]
    fn list_output_empty_shows_nothing_installed() {
        let r = ListOutput {
            agents: BTreeMap::new(),
            skills: BTreeMap::new(),
        };
        assert_eq!(r.to_string(), "Nothing installed.\n");
    }

    #[test]
    fn list_output_with_agents_shows_section() {
        let mut agents = BTreeMap::new();
        agents.insert(InstallScope::Global, vec![make_row("my-agent")]);
        let r = ListOutput {
            agents,
            skills: BTreeMap::new(),
        };
        let out = r.to_string();
        assert!(out.contains("Global agents:"));
        assert!(out.contains("my-agent"));
    }
}
