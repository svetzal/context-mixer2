use std::fmt;

use crate::outdated::OutdatedReport;

use super::util;

impl fmt::Display for OutdatedReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = &self.0;
        if rows.is_empty() {
            return writeln!(f, "Everything is up to date.");
        }

        let mapped_rows: Vec<Vec<String>> = rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.kind.to_string(),
                    r.installed_version.as_deref().unwrap_or("unversioned").to_string(),
                    r.available_version.as_deref().unwrap_or("unversioned").to_string(),
                    r.source.clone(),
                    if r.locally_modified {
                        format!("{} (modified)", r.status.label())
                    } else {
                        r.status.label().to_string()
                    },
                ]
            })
            .collect();
        writeln!(
            f,
            "{}Update with: cmx <kind> update <name> (or cmx skill update --all)",
            util::table_or_empty(
                "Everything is up to date.",
                vec!["Name", "Type", "Installed", "Available", "Source", "Status"],
                6,
                mapped_rows,
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outdated::{OutdatedRow, OutdatedStatus};
    use crate::types::{ArtifactKind, InstallScope};

    // --- Step 8: OutdatedReport ---

    #[test]
    fn outdated_report_empty_up_to_date() {
        let r = OutdatedReport(vec![]);
        assert_eq!(r.to_string(), "Everything is up to date.\n");
    }

    #[test]
    fn outdated_report_populated_shows_rows() {
        let r = OutdatedReport(vec![OutdatedRow {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: InstallScope::Global,
            installed_version: Some("1.0.0".to_string()),
            available_version: Some("2.0.0".to_string()),
            source: "guidelines".to_string(),
            status: OutdatedStatus::Outdated,
            locally_modified: true,
        }]);
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("2.0.0"));
        assert!(out.contains("outdated (modified)"));
        assert!(out.contains("Update with: cmx <kind> update <name>"));
    }

    #[test]
    fn outdated_report_unversioned_rows_use_explicit_words() {
        let r = OutdatedReport(vec![OutdatedRow {
            name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            scope: InstallScope::Global,
            installed_version: None,
            available_version: None,
            source: "guidelines".to_string(),
            status: OutdatedStatus::Changed,
            locally_modified: false,
        }]);
        let out = r.to_string();
        assert!(out.contains("unversioned"));
        assert!(!out.contains(" - "));
    }
}
