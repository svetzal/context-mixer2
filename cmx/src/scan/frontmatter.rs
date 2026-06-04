use std::path::PathBuf;

use crate::types::{Artifact, ArtifactKind, Deprecation};

pub(crate) struct Frontmatter {
    pub(crate) description: String,
    pub(crate) version: Option<String>,
    pub(crate) deprecation: Option<Deprecation>,
}

pub(crate) fn artifact_from_frontmatter(
    kind: ArtifactKind,
    name: String,
    path: PathBuf,
    fm: Frontmatter,
) -> Artifact {
    Artifact {
        kind,
        name,
        description: fm.description,
        path,
        version: fm.version,
        deprecation: fm.deprecation,
    }
}

fn parse_deprecation(fm_text: &str) -> Option<Deprecation> {
    let deprecated = extract_field(fm_text, "deprecated")?;
    if deprecated != "true" {
        return None;
    }
    Some(Deprecation {
        reason: extract_field(fm_text, "deprecated_reason"),
        replacement: extract_field(fm_text, "deprecated_replacement"),
    })
}

/// Split YAML frontmatter from content. Returns `(Some(frontmatter), body)` when
/// `---` fences are found at line boundaries, or `(None, full_content)` otherwise.
/// Handles both LF and CRLF line endings. Unterminated blocks return `(None, full_content)`.
pub(crate) fn split_frontmatter_and_body(content: &str) -> (Option<String>, &str) {
    let Some(rest) = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n"))
    else {
        return (None, content);
    };

    let mut search_start = 0;
    while let Some(idx) = rest[search_start..].find("---") {
        let abs = search_start + idx;
        let at_line_start = abs == 0 || rest.as_bytes()[abs - 1] == b'\n';
        let after = &rest[abs + 3..];
        let ends_line = after.is_empty() || after.starts_with('\n') || after.starts_with('\r');
        if at_line_start && ends_line {
            let frontmatter = rest[..abs].to_string();
            let body =
                after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
            return (Some(frontmatter), body);
        }
        search_start = abs + 3;
    }

    (None, content)
}

pub(crate) fn parse_frontmatter_str(content: &str) -> Option<Frontmatter> {
    parse_frontmatter_impl(content, &[])
}

pub(crate) fn parse_agent_frontmatter_str(content: &str) -> Option<Frontmatter> {
    parse_frontmatter_impl(content, &["name", "description"])
}

fn parse_frontmatter_impl(content: &str, required_fields: &[&str]) -> Option<Frontmatter> {
    let (fm_opt, _) = split_frontmatter_and_body(content);
    let fm_text = fm_opt.as_deref()?;
    for field in required_fields {
        if !fm_text.lines().any(|l| l.starts_with(&format!("{field}:"))) {
            return None;
        }
    }
    Some(Frontmatter {
        description: extract_field(fm_text, "description").unwrap_or_default(),
        version: extract_version(fm_text),
        deprecation: parse_deprecation(fm_text),
    })
}

pub fn extract_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let mut lines = frontmatter.lines();
    let header = lines.find(|l| l.starts_with(&prefix))?;
    let inline = header[prefix.len()..].trim();

    // YAML block scalar: `description: >` (folded) or `description: |` (literal),
    // optionally with chomping/indent indicators (`>-`, `|+`, …). The value is the
    // indented lines that follow, joined per the style. Without this, a multi-line
    // description collapses to just the `>`/`|` indicator.
    if let Some(folded) = block_scalar_style(inline) {
        let body: Vec<&str> = lines
            .take_while(|l| l.trim().is_empty() || l.starts_with([' ', '\t']))
            .map(str::trim)
            .collect();
        let joined = if folded {
            body.join(" ")
        } else {
            body.join("\n")
        };
        let joined = joined.trim().to_string();
        return (!joined.is_empty()).then_some(joined);
    }

    let value = inline.trim_matches('"').to_string();
    (!value.is_empty()).then_some(value)
}

/// If `inline` is a YAML block-scalar indicator (`>` folded or `|` literal, with
/// optional chomping/indent indicators), return `true` for folded, `false` for
/// literal. Returns `None` when it's an ordinary inline value.
fn block_scalar_style(inline: &str) -> Option<bool> {
    let mut chars = inline.chars();
    let style = chars.next()?;
    if style != '>' && style != '|' {
        return None;
    }
    // Everything after the indicator must be chomping/indent indicators (or a
    // comment) — otherwise it's an inline value that merely starts with `>`/`|`.
    let rest = inline[1..].trim();
    let indicators_only = rest.is_empty()
        || rest.starts_with('#')
        || rest.chars().all(|c| matches!(c, '-' | '+' | '0'..='9'));
    indicators_only.then_some(style == '>')
}

/// Extract a field nested under `metadata:` in YAML frontmatter.
/// Matches indented lines (spaces or tabs) under the `metadata:` block.
pub fn extract_metadata_field(frontmatter: &str, key: &str) -> Option<String> {
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if line.starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        if in_metadata {
            let trimmed = line.trim_start();
            // A non-indented, non-empty line means we've left the metadata block
            if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
                return None;
            }
            let prefix = format!("{key}:");
            if trimmed.starts_with(&prefix) {
                let value = trimmed[prefix.len()..].trim().trim_matches('"').to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Extract version from frontmatter, checking root-level first then metadata block.
pub fn extract_version(frontmatter: &str) -> Option<String> {
    extract_field(frontmatter, "version").or_else(|| extract_metadata_field(frontmatter, "version"))
}

/// Extract the version from an installed artifact's file content.
/// For agents, pass the .md file content. For skills, pass the SKILL.md content.
pub fn extract_version_from_content(content: &str) -> Option<String> {
    let (fm_opt, _) = split_frontmatter_and_body(content);
    extract_version(fm_opt.as_deref()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_field ---

    #[test]
    fn extract_field_basic() {
        let text = "name: my-agent\ndescription: A thing";
        assert_eq!(extract_field(text, "name"), Some("my-agent".to_string()));
    }

    #[test]
    fn extract_field_quoted_value() {
        let text = "name: \"my-agent\"";
        assert_eq!(extract_field(text, "name"), Some("my-agent".to_string()));
    }

    #[test]
    fn extract_field_not_present() {
        let text = "name: my-agent";
        assert_eq!(extract_field(text, "version"), None);
    }

    #[test]
    fn extract_field_empty_value_filtered() {
        let text = "name: ";
        assert_eq!(extract_field(text, "name"), None);
    }

    #[test]
    fn extract_field_extra_whitespace_trimmed() {
        let text = "name:   spaced-value   ";
        assert_eq!(extract_field(text, "name"), Some("spaced-value".to_string()));
    }

    #[test]
    fn extract_field_multiple_fields_picks_correct_one() {
        let text = "name: my-agent\ndescription: A thing\nversion: 1.0.0";
        assert_eq!(extract_field(text, "description"), Some("A thing".to_string()));
    }

    #[test]
    fn extract_field_no_prefix_collision() {
        // key "name" must not match line "namespace: foo"
        let text = "namespace: foo";
        assert_eq!(extract_field(text, "name"), None);
    }

    #[test]
    fn extract_field_folded_block_scalar_joins_with_spaces() {
        let text = "name: lint\ndescription: >\n  Run markdownlint to fix files.\n  Use it whenever a .md file changes.\nversion: 1.0.0";
        assert_eq!(
            extract_field(text, "description"),
            Some("Run markdownlint to fix files. Use it whenever a .md file changes.".to_string())
        );
        // The following key is unaffected.
        assert_eq!(extract_field(text, "version"), Some("1.0.0".to_string()));
    }

    #[test]
    fn extract_field_literal_block_scalar_keeps_newlines() {
        let text = "description: |\n  line one\n  line two\n";
        assert_eq!(extract_field(text, "description"), Some("line one\nline two".to_string()));
    }

    #[test]
    fn extract_field_folded_block_scalar_with_chomping_indicator() {
        let text = "description: >-\n  folded text here\n";
        assert_eq!(extract_field(text, "description"), Some("folded text here".to_string()));
    }

    #[test]
    fn extract_field_inline_value_starting_with_gt_is_not_a_block_scalar() {
        // A genuine inline value that happens to start with `>` (not a bare
        // indicator) is taken verbatim, not treated as a block scalar.
        let text = "description: >= 2.0 required";
        assert_eq!(extract_field(text, "description"), Some(">= 2.0 required".to_string()));
    }

    // --- extract_metadata_field ---

    #[test]
    fn extract_metadata_field_basic() {
        let text = "metadata:\n  version: \"1.3.2\"\n  author: Test";
        assert_eq!(extract_metadata_field(text, "version"), Some("1.3.2".to_string()));
    }

    #[test]
    fn extract_metadata_field_unquoted() {
        let text = "metadata:\n  version: 1.0.0";
        assert_eq!(extract_metadata_field(text, "version"), Some("1.0.0".to_string()));
    }

    #[test]
    fn extract_metadata_field_not_in_metadata() {
        let text = "name: my-agent\nversion: 1.0.0";
        assert_eq!(extract_metadata_field(text, "version"), None);
    }

    #[test]
    fn extract_metadata_field_no_metadata_block() {
        let text = "name: my-agent\ndescription: stuff";
        assert_eq!(extract_metadata_field(text, "version"), None);
    }

    #[test]
    fn extract_metadata_field_stops_at_next_root_key() {
        let text = "metadata:\n  author: Test\nother_key: value\n  version: 1.0.0";
        // version appears after other_key, so it's not under metadata
        assert_eq!(extract_metadata_field(text, "version"), None);
    }

    #[test]
    fn extract_metadata_field_empty_value_filtered() {
        let text = "metadata:\n  version: ";
        assert_eq!(extract_metadata_field(text, "version"), None);
    }

    #[test]
    fn extract_metadata_field_with_tabs() {
        let text = "metadata:\n\tversion: 2.0.0";
        assert_eq!(extract_metadata_field(text, "version"), Some("2.0.0".to_string()));
    }

    // --- extract_version (root-level vs metadata fallback) ---

    #[test]
    fn extract_version_prefers_root_level() {
        let text = "version: 1.0.0\nmetadata:\n  version: \"2.0.0\"";
        assert_eq!(extract_version(text), Some("1.0.0".to_string()));
    }

    #[test]
    fn extract_version_falls_back_to_metadata() {
        let text = "name: my-agent\nmetadata:\n  version: \"1.3.2\"";
        assert_eq!(extract_version(text), Some("1.3.2".to_string()));
    }

    #[test]
    fn extract_version_none_when_absent_everywhere() {
        let text = "name: my-agent\ndescription: stuff";
        assert_eq!(extract_version(text), None);
    }

    // --- parse_deprecation ---

    #[test]
    fn parse_deprecation_true_with_reason_and_replacement() {
        let text =
            "deprecated: true\ndeprecated_reason: Too old\ndeprecated_replacement: new-agent";
        let dep = parse_deprecation(text).expect("expected Some");
        assert_eq!(dep.reason.as_deref(), Some("Too old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-agent"));
    }

    #[test]
    fn parse_deprecation_true_no_reason_or_replacement() {
        let text = "deprecated: true";
        let dep = parse_deprecation(text).expect("expected Some");
        assert!(dep.reason.is_none());
        assert!(dep.replacement.is_none());
    }

    #[test]
    fn parse_deprecation_false_returns_none() {
        let text = "deprecated: false";
        assert!(parse_deprecation(text).is_none());
    }

    #[test]
    fn parse_deprecation_absent_returns_none() {
        let text = "name: my-agent\ndescription: A thing";
        assert!(parse_deprecation(text).is_none());
    }

    // --- split_frontmatter_and_body ---

    #[test]
    fn split_frontmatter_and_body_extracts_frontmatter_and_body() {
        let (fm, body) = split_frontmatter_and_body("---\nkey: value\n---\n# body");
        assert_eq!(fm.as_deref(), Some("key: value\n"));
        assert_eq!(body, "# body");
    }

    #[test]
    fn split_frontmatter_and_body_no_opening_delimiter_returns_none() {
        let content = "key: value\n---\n# body";
        let (fm, body) = split_frontmatter_and_body(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn split_frontmatter_and_body_unterminated_returns_none() {
        let content = "---\nkey: value\n# body";
        let (fm, body) = split_frontmatter_and_body(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn split_frontmatter_and_body_handles_crlf() {
        let (fm, body) = split_frontmatter_and_body("---\r\nkey: value\r\n---\r\nBody\r\n");
        assert_eq!(fm.as_deref(), Some("key: value\r\n"));
        assert_eq!(body, "Body\r\n");
    }

    #[test]
    fn split_frontmatter_and_body_dashes_not_at_line_boundary_are_ignored() {
        // "------" is not a valid opener — requires exactly ---\n or ---\r\n
        let content = "------\n# body";
        let (fm, body) = split_frontmatter_and_body(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    // --- parse_frontmatter_str ---

    #[test]
    fn parse_frontmatter_str_valid_all_fields() {
        let content = "---\ndescription: Test skill\nversion: 1.2.3\n---\n# content";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "Test skill");
        assert_eq!(fm.version.as_deref(), Some("1.2.3"));
        assert!(fm.deprecation.is_none());
    }

    #[test]
    fn parse_frontmatter_str_no_delimiters_returns_none() {
        let content = "description: Test skill\n# content";
        assert!(parse_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_frontmatter_str_missing_closing_delimiter_returns_none() {
        let content = "---\ndescription: Test skill\n# content";
        // "---\n" then rest="description: Test skill\n# content", no "---" found
        assert!(parse_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_frontmatter_str_without_version() {
        let content = "---\ndescription: No version here\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "No version here");
        assert!(fm.version.is_none());
    }

    #[test]
    fn parse_frontmatter_str_with_deprecation() {
        let content =
            "---\ndescription: Old skill\ndeprecated: true\ndeprecated_reason: Replaced\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        let dep = fm.deprecation.expect("expected deprecation");
        assert_eq!(dep.reason.as_deref(), Some("Replaced"));
    }

    #[test]
    fn parse_frontmatter_str_metadata_version() {
        let content =
            "---\ndescription: Test skill\nmetadata:\n  version: \"2.1.0\"\n  author: Test\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn parse_frontmatter_str_root_version_preferred_over_metadata() {
        let content =
            "---\ndescription: Test\nversion: 1.0.0\nmetadata:\n  version: \"2.0.0\"\n---\n";
        let fm = parse_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.version.as_deref(), Some("1.0.0"));
    }

    // --- parse_agent_frontmatter_str ---

    #[test]
    fn parse_agent_frontmatter_str_valid() {
        let content = "---\nname: my-agent\ndescription: Does things\n---\n# body";
        let fm = parse_agent_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.description, "Does things");
    }

    #[test]
    fn parse_agent_frontmatter_str_missing_name_returns_none() {
        let content = "---\ndescription: Does things\n---\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_agent_frontmatter_str_missing_description_returns_none() {
        let content = "---\nname: my-agent\n---\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_agent_frontmatter_str_no_delimiters_returns_none() {
        let content = "name: my-agent\ndescription: Does things\n# body";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }

    #[test]
    fn parse_agent_frontmatter_str_metadata_version() {
        let content = "---\nname: my-agent\ndescription: Does things\nmetadata:\n  version: \"1.3.2\"\n  author: Test\n---\n# body";
        let fm = parse_agent_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.version.as_deref(), Some("1.3.2"));
    }

    #[test]
    fn parse_agent_frontmatter_str_root_version_preferred_over_metadata() {
        let content = "---\nname: my-agent\ndescription: Does things\nversion: 1.0.0\nmetadata:\n  version: \"2.0.0\"\n---\n# body";
        let fm = parse_agent_frontmatter_str(content).expect("expected Some");
        assert_eq!(fm.version.as_deref(), Some("1.0.0"));
    }
}
