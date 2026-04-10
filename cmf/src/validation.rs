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

/// Print formatted validation output grouped by level.
///
/// Errors are printed first, then warnings. If no issues exist, prints
/// a success message.
pub fn print_validation_issues(issues: &[ValidationIssue]) {
    if issues.is_empty() {
        println!("All plugins valid.");
        return;
    }

    let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
    let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

    if !errors.is_empty() {
        println!("Errors:");
        for issue in &errors {
            println!("  {}: {}", issue.context, issue.message);
        }
    }

    if !warnings.is_empty() {
        println!("Warnings:");
        for issue in &warnings {
            println!("  {}: {}", issue.context, issue.message);
        }
    }
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
