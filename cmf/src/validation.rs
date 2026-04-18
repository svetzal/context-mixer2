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

/// Format validation output grouped by level.
///
/// Errors are formatted first, then warnings. If no issues exist, returns
/// a success message.
pub fn format_validation_issues(issues: &[ValidationIssue]) -> String {
    use std::fmt::Write as FmtWrite;

    if issues.is_empty() {
        return "All plugins valid.\n".to_string();
    }

    let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
    let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

    let mut out = String::new();

    if !errors.is_empty() {
        out.push_str("Errors:\n");
        for issue in &errors {
            let _ = writeln!(out, "  {}: {}", issue.context, issue.message);
        }
    }

    if !warnings.is_empty() {
        out.push_str("Warnings:\n");
        for issue in &warnings {
            let _ = writeln!(out, "  {}: {}", issue.context, issue.message);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_level_equality() {
        assert_eq!(IssueLevel::Error, IssueLevel::Error);
        assert_eq!(IssueLevel::Warning, IssueLevel::Warning);
        assert_ne!(IssueLevel::Error, IssueLevel::Warning);
    }
}
