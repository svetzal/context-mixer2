/// Shared validation types used across plugin, marketplace, and facet validation.
use std::path::Path;

use anyhow::Result;
use cmx::gateway::Filesystem;

/// Parse a JSON string, returning a validated value or a malformed-file issue.
///
/// Pure function — no I/O. Returns `(Some(value), [])` on success or
/// `(None, [malformed issue])` on parse error.
pub fn parse_and_validate_json<T: serde::de::DeserializeOwned>(
    raw: &str,
    context: &str,
    file_label: &str,
) -> (Option<T>, Vec<ValidationIssue>) {
    match serde_json::from_str(raw) {
        Ok(v) => (Some(v), vec![]),
        Err(e) => (
            None,
            vec![ValidationIssue::error(
                context,
                format!("{file_label} is malformed: {e}"),
            )],
        ),
    }
}

/// Load and parse a JSON file, collecting validation issues for missing,
/// unreadable, or malformed files.
///
/// Returns `(Some(value), [])` on success or `(None, [issue])` on failure.
/// Callers should return early when the returned issue list is non-empty.
pub fn load_and_validate_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    context: &str,
    file_label: &str,
    fs: &dyn Filesystem,
) -> Result<(Option<T>, Vec<ValidationIssue>)> {
    if !fs.exists(path) {
        return Ok((
            None,
            vec![ValidationIssue::error(
                context,
                format!("{file_label} is missing"),
            )],
        ));
    }

    let Ok(raw) = fs.read_to_string(path) else {
        return Ok((
            None,
            vec![ValidationIssue::error(
                context,
                format!("{file_label} could not be read"),
            )],
        ));
    };

    Ok(parse_and_validate_json(&raw, context, file_label))
}

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

    // --- parse_and_validate_json (pure) ---

    #[test]
    fn parse_and_validate_json_valid_returns_value_and_no_issues() {
        let raw = r#"{"name": "test"}"#;
        let (value, issues): (Option<serde_json::Value>, _) =
            parse_and_validate_json(raw, "ctx", "file.json");
        assert!(value.is_some(), "valid JSON must return Some(value)");
        assert!(issues.is_empty(), "valid JSON must produce no issues");
    }

    #[test]
    fn parse_and_validate_json_malformed_returns_none_and_error_issue() {
        let raw = "{ not valid json";
        let (value, issues): (Option<serde_json::Value>, _) =
            parse_and_validate_json(raw, "ctx", "file.json");
        assert!(value.is_none(), "malformed JSON must return None");
        assert_eq!(issues.len(), 1, "malformed JSON must produce exactly one issue");
        assert_eq!(issues[0].level, IssueLevel::Error);
        assert!(issues[0].message.contains("malformed"), "issue message must say 'malformed'");
    }

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
