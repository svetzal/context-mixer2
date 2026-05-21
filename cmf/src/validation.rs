/// Shared validation types used across plugin, marketplace, and facet validation.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub level: IssueLevel,
    pub context: String,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Error,
            context: context.into(),
            message: message.into(),
        }
    }

    pub fn warning(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Warning,
            context: context.into(),
            message: message.into(),
        }
    }
}

pub struct ValidationReport(pub Vec<ValidationIssue>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_level_equality() {
        assert_eq!(IssueLevel::Error, IssueLevel::Error);
        assert_eq!(IssueLevel::Warning, IssueLevel::Warning);
        assert_ne!(IssueLevel::Error, IssueLevel::Warning);
    }

    // --- Display for ValidationReport ---

    #[test]
    fn validation_report_display_empty() {
        let out = ValidationReport(vec![]).to_string();
        assert_eq!(out, "All plugins valid.\n");
    }

    #[test]
    fn validation_report_display_errors_only() {
        let issues = vec![ValidationIssue::error(
            "plugin/alpha",
            "Missing description",
        )];
        let out = ValidationReport(issues).to_string();
        assert!(out.contains("Errors:"));
        assert!(out.contains("plugin/alpha"));
        assert!(out.contains("Missing description"));
        assert!(!out.contains("Warnings:"));
    }

    #[test]
    fn validation_report_display_warnings_only() {
        let issues = vec![ValidationIssue::warning(
            "plugin/beta",
            "Version not semver",
        )];
        let out = ValidationReport(issues).to_string();
        assert!(out.contains("Warnings:"));
        assert!(!out.contains("Errors:"));
    }

    #[test]
    fn validation_report_display_both() {
        let issues = vec![
            ValidationIssue::error("plugin/alpha", "Missing description"),
            ValidationIssue::warning("plugin/beta", "Version not semver"),
        ];
        let out = ValidationReport(issues).to_string();
        assert!(out.contains("Errors:"));
        assert!(out.contains("Warnings:"));
    }
}
