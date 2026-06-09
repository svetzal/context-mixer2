use std::fmt;

use cmx::table::section;

use crate::validation::{IssueLevel, ValidationReport};

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let issues = &self.0;
        if issues.is_empty() {
            return writeln!(f, "All plugins valid.");
        }

        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

        if !errors.is_empty() {
            let lines: Vec<String> =
                errors.iter().map(|i| format!("{}: {}", i.context, i.message)).collect();
            write!(f, "{}", section("Errors:", &lines))?;
        }

        if !warnings.is_empty() {
            let lines: Vec<String> =
                warnings.iter().map(|i| format!("{}: {}", i.context, i.message)).collect();
            write!(f, "{}", section("Warnings:", &lines))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::validation::{IssueLevel, ValidationIssue, ValidationReport};

    #[test]
    fn validation_report_display_clean() {
        let report = ValidationReport(vec![]);
        assert_eq!(report.to_string(), "All plugins valid.\n");
    }

    #[test]
    fn validation_report_display_with_errors_and_warnings() {
        let issues = vec![
            ValidationIssue {
                level: IssueLevel::Error,
                context: "p1".to_string(),
                message: "bad".to_string(),
            },
            ValidationIssue {
                level: IssueLevel::Warning,
                context: "p2".to_string(),
                message: "iffy".to_string(),
            },
        ];
        let out = ValidationReport(issues).to_string();
        assert!(out.contains("Errors:"));
        assert!(out.contains("bad"));
        assert!(out.contains("Warnings:"));
        assert!(out.contains("iffy"));
    }
}
