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
