use std::fmt;

use crate::outdated::OutdatedReport;

use super::util;

impl fmt::Display for OutdatedReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = &self.0;
        let mapped_rows: Vec<Vec<String>> = rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.kind.to_string(),
                    r.installed_version.clone(),
                    r.available_version.clone(),
                    r.source.clone(),
                    r.status.clone(),
                ]
            })
            .collect();
        write!(
            f,
            "{}",
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
    use crate::outdated::OutdatedRow;
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
            installed_version: "1.0.0".to_string(),
            available_version: "2.0.0".to_string(),
            source: "guidelines".to_string(),
            status: "update".to_string(),
        }]);
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("2.0.0"));
    }
}
