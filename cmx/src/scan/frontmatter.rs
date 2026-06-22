use std::path::PathBuf;

use serde_yaml_ng::{Mapping, Value};

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

/// Normalize frontmatter before YAML parsing to handle common non-spec patterns
/// found in real-world artifact files:
///
/// 1. **Tab indentation** — YAML disallows tabs; replace leading tabs with two
///    spaces so indented blocks (e.g. `metadata:\n\tversion:`) parse correctly.
///
/// 2. **Unquoted `>`/`|` inline values** — `description: >= 2.0` is technically
///    invalid YAML because `>` opens a block-scalar context.  When a key's value
///    starts with `>` or `|` but the rest of the line isn't a valid block-scalar
///    header (i.e. it has non-indicator characters after the initial `>`/`|`),
///    we single-quote the whole value so the YAML library treats it as a plain
///    string instead of a block-scalar indicator.
fn normalize_frontmatter(frontmatter: &str) -> String {
    let mut out = String::with_capacity(frontmatter.len());
    for line in frontmatter.lines() {
        // Replace leading tabs with two spaces each.
        let normalized_indent: String = {
            let n_tabs = line.chars().take_while(|&c| c == '\t').count();
            if n_tabs > 0 {
                " ".repeat(n_tabs * 2) + line[n_tabs..].trim_start_matches('\t')
            } else {
                line.to_string()
            }
        };

        // Quote inline values that would be misinterpreted as block-scalar indicators.
        let fixed = fix_unquoted_block_indicator_value(&normalized_indent);
        out.push_str(&fixed);
        out.push('\n');
    }
    out
}

/// If `line` is a YAML mapping entry whose value starts with `>` or `|` but is
/// NOT a valid block-scalar header (i.e. extra non-indicator content follows on
/// the same line), wrap the value in single quotes so the YAML parser treats it
/// as a plain scalar.
fn fix_unquoted_block_indicator_value(line: &str) -> String {
    // Match a bare key: value line at the start (no leading indent for root keys,
    // or indented for nested keys).
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return line.to_string();
    }

    // Find `key:` followed by a space and value.
    let Some(colon_pos) = trimmed.find(':') else {
        return line.to_string();
    };
    let after_colon = &trimmed[colon_pos + 1..];
    // Must have a space after colon (not a nested mapping key).
    let Some(value_str) = after_colon.strip_prefix(' ') else {
        return line.to_string();
    };

    // Only act when value starts with `>` or `|`.
    let Some(first @ ('>' | '|')) = value_str.chars().next() else {
        return line.to_string();
    };

    // A real block-scalar header has ONLY optional chomping/indent indicators
    // after the `>`/`|`, then end-of-line or a comment.  If the rest has
    // other chars, it's an inline plain scalar starting with `>`/`|`.
    let rest = value_str[first.len_utf8()..].trim_end();
    let is_block_scalar_header = rest.is_empty()
        || rest.starts_with('#')
        || rest.chars().all(|c| matches!(c, '-' | '+' | '0'..='9'));
    if is_block_scalar_header {
        return line.to_string();
    }

    // It's an inline plain scalar that starts with a block-scalar indicator char.
    // Re-emit the line with the value single-quoted.
    let indent = &line[..line.len() - trimmed.len()];
    let key = &trimmed[..colon_pos];
    // Escape any single quotes in the value.
    let escaped = value_str.replace('\'', "''");
    format!("{indent}{key}: '{escaped}'")
}

/// Parse a YAML frontmatter string into a `Mapping`. Returns `None` if the
/// input is neither valid YAML nor recoverable by the lenient fallback.
///
/// Strict YAML is tried first (after [`normalize_frontmatter`]). Only when that
/// fails do we fall back to [`lenient_mapping`], so well-formed frontmatter is
/// unaffected and keeps its exact YAML semantics.
fn parse_yaml_mapping(frontmatter: &str) -> Option<Mapping> {
    let normalized = normalize_frontmatter(frontmatter);
    match serde_yaml_ng::from_str::<Value>(&normalized) {
        Ok(Value::Mapping(m)) => Some(m),
        _ => lenient_mapping(frontmatter),
    }
}

/// Best-effort frontmatter parse for input that strict YAML rejects.
///
/// Real-world skill/agent frontmatter sometimes carries an unquoted, multi-
/// paragraph `description:` value broken by a blank line — accepted by Claude
/// Code's loader but invalid YAML (a plain scalar can't resume after a blank
/// line at column 0). Rather than silently dropping the whole artifact, recover
/// a flat top-level mapping by scanning `key: value` lines: a value continues
/// across following lines that are not themselves top-level keys (blank lines
/// included) and is whitespace-joined into a single line.
///
/// Invoked only when strict parsing fails, so well-formed frontmatter never
/// takes this looser path. Nested mappings (e.g. a `metadata:` block) are not
/// reconstructed — indented children fold into the parent value — which is an
/// acceptable trade for the malformed inputs this rescues.
fn lenient_mapping(frontmatter: &str) -> Option<Mapping> {
    /// Split a line into `(key, inline_value)` when it is a top-level mapping
    /// entry: at column 0, an identifier key, then `:` followed by a space or
    /// end-of-line. Returns `None` for indented lines, blank lines, and prose
    /// (so `http://x` mid-value isn't mistaken for a key).
    fn top_level_key(line: &str) -> Option<(&str, &str)> {
        if line.starts_with([' ', '\t']) {
            return None;
        }
        let colon = line.find(':')?;
        let key = &line[..colon];
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        let rest = &line[colon + 1..];
        match rest.strip_prefix(' ') {
            Some(value) => Some((key, value)),
            None if rest.is_empty() => Some((key, "")),
            None => None,
        }
    }

    /// Collapse the collected value lines into a single whitespace-normalized
    /// string and record it (dropping empty values, e.g. bare `metadata:`).
    fn flush(mapping: &mut Mapping, entry: Option<(String, Vec<String>)>) {
        if let Some((key, parts)) = entry {
            let value = parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
            if !value.is_empty() {
                mapping.insert(Value::String(key), Value::String(value));
            }
        }
    }

    let mut mapping = Mapping::new();
    let mut current: Option<(String, Vec<String>)> = None;

    for line in frontmatter.lines() {
        if let Some((key, value)) = top_level_key(line) {
            flush(&mut mapping, current.take());
            current = Some((key.to_string(), vec![value.to_string()]));
        } else if let Some((_, parts)) = current.as_mut() {
            parts.push(line.to_string());
        }
    }
    flush(&mut mapping, current.take());

    (!mapping.is_empty()).then_some(mapping)
}

/// Convert a `Value` scalar to a non-empty `String`, or `None` for null/empty.
fn scalar_to_string(value: &Value) -> Option<String> {
    let s = match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let trimmed = s.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn parse_deprecation(fm_text: &str) -> Option<Deprecation> {
    let mapping = parse_yaml_mapping(fm_text)?;
    let deprecated_key = Value::String("deprecated".to_string());
    let deprecated = mapping.get(&deprecated_key)?;
    // Accept both `true` (YAML bool) and the string `"true"` for resilience.
    let is_deprecated = matches!(deprecated, Value::Bool(true))
        || matches!(deprecated, Value::String(s) if s == "true");
    if !is_deprecated {
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
pub fn split_frontmatter_and_body(content: &str) -> (Option<String>, &str) {
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
    let mapping = parse_yaml_mapping(fm_text)?;
    for field in required_fields {
        if !mapping.contains_key(Value::String((*field).to_string())) {
            return None;
        }
    }
    Some(Frontmatter {
        description: extract_field(fm_text, "description").unwrap_or_default(),
        version: extract_version(fm_text),
        deprecation: parse_deprecation(fm_text),
    })
}

/// Extract a top-level field from YAML frontmatter as a string.
/// Returns `None` when the key is absent or the value is empty/null/non-scalar.
pub fn extract_field(frontmatter: &str, key: &str) -> Option<String> {
    let mapping = parse_yaml_mapping(frontmatter)?;
    let value = mapping.get(Value::String(key.to_string()))?;
    scalar_to_string(value)
}

/// Extract a field nested under `metadata:` in YAML frontmatter.
/// Handles both block and flow mapping styles.
pub fn extract_metadata_field(frontmatter: &str, key: &str) -> Option<String> {
    let mapping = parse_yaml_mapping(frontmatter)?;
    let metadata_key = Value::String("metadata".to_string());
    let metadata = mapping.get(&metadata_key)?;
    let Value::Mapping(sub) = metadata else {
        return None;
    };
    let value = sub.get(Value::String(key.to_string()))?;
    scalar_to_string(value)
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

    // --- parse_yaml_mapping ---

    #[test]
    fn parse_yaml_mapping_valid_returns_mapping() {
        assert!(parse_yaml_mapping("name: foo\nversion: 1.0.0").is_some());
    }

    #[test]
    fn parse_yaml_mapping_unrecoverable_returns_none() {
        // No top-level `key:` line anywhere — neither strict YAML nor the
        // lenient fallback can make a mapping of it.
        assert!(parse_yaml_mapping("  just indented prose, no keys").is_none());
    }

    #[test]
    fn parse_yaml_mapping_yaml_invalid_but_recoverable_falls_back() {
        // Strict YAML rejects an unclosed flow sequence; the fallback still
        // recovers the key with a best-effort scalar value rather than dropping
        // the whole frontmatter.
        let mapping =
            parse_yaml_mapping("key: [unclosed bracket").expect("fallback recovers the key");
        assert!(mapping.contains_key(Value::String("key".to_string())));
    }

    // --- lenient fallback for YAML-invalid frontmatter ---

    #[test]
    fn lenient_recovers_multiparagraph_description() {
        // An unquoted description broken by a blank line into a second paragraph
        // is invalid YAML, but Claude Code accepts it. The fallback recovers the
        // skill instead of dropping it, joining the paragraphs into one line.
        let text = "name: personal-finance\n\
                    description: First paragraph runs long.\n\
                    \n\
                    Second paragraph after a blank line.";
        let mapping = parse_yaml_mapping(text).expect("fallback should recover a mapping");
        assert!(mapping.contains_key(Value::String("name".to_string())));
        assert_eq!(
            extract_field(text, "description").as_deref(),
            Some("First paragraph runs long. Second paragraph after a blank line."),
        );
        assert_eq!(extract_field(text, "name").as_deref(), Some("personal-finance"));
    }

    #[test]
    fn lenient_does_not_engage_for_valid_yaml() {
        // A genuine block scalar must keep its YAML semantics (newlines), proving
        // the fallback only runs when strict parsing fails.
        let text = "description: |\n  line one\n  line two\n";
        assert_eq!(extract_field(text, "description"), Some("line one\nline two".to_string()));
    }

    #[test]
    fn lenient_skill_frontmatter_round_trips_through_parser() {
        let content = "---\nname: personal-finance\n\
                       description: Maintain the ledger.\n\
                       \n\
                       For the CLI itself, see the gilt skill.\n---\n# body";
        let fm = parse_frontmatter_str(content).expect("skill should parse via fallback");
        assert_eq!(fm.description, "Maintain the ledger. For the CLI itself, see the gilt skill.");
    }

    #[test]
    fn lenient_agent_frontmatter_recovers_required_fields() {
        let content = "---\nname: my-agent\n\
                       description: Does things across\n\
                       \n\
                       multiple paragraphs.\n---\n# body";
        let fm = parse_agent_frontmatter_str(content).expect("agent should parse via fallback");
        assert_eq!(fm.description, "Does things across multiple paragraphs.");
    }

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
    fn extract_field_single_quoted_value() {
        let text = "name: 'my-agent'";
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

    #[test]
    fn extract_field_inline_comment_stripped() {
        let text = "name: x  # a comment";
        assert_eq!(extract_field(text, "name"), Some("x".to_string()));
    }

    #[test]
    fn extract_field_numeric_scalar() {
        let text = "count: 42";
        assert_eq!(extract_field(text, "count"), Some("42".to_string()));
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

    #[test]
    fn extract_metadata_field_flow_mapping() {
        let text = "metadata: { version: 1.2.3, author: Test }";
        assert_eq!(extract_metadata_field(text, "version"), Some("1.2.3".to_string()));
    }

    #[test]
    fn extract_metadata_field_nested_quoted() {
        let text = "metadata:\n  version: \"2.5.0\"\n  author: 'Alice'";
        assert_eq!(extract_metadata_field(text, "author"), Some("Alice".to_string()));
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

    #[test]
    fn parse_deprecation_bool_true_honored() {
        // YAML `true` is a bool, not the string "true"
        let text = "deprecated: true\ndeprecated_reason: Old";
        let dep = parse_deprecation(text).expect("expected Some");
        assert_eq!(dep.reason.as_deref(), Some("Old"));
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

    #[test]
    fn parse_frontmatter_impl_no_prefix_collision_on_required_field() {
        // "namespace" field must not satisfy the "name" requirement
        let content = "---\nnamespace: foo\ndescription: bar\n---\n";
        assert!(parse_agent_frontmatter_str(content).is_none());
    }
}
