//! Transform a cmx markdown agent into a codex CLI subagent TOML document.
//!
//! cmx agents are markdown files with YAML frontmatter (`name`, `description`,
//! optional `model`, …) followed by a body that serves as the system prompt.
//! The `OpenAI` Codex CLI instead defines subagents as standalone TOML files with
//! `name`, `description`, and `developer_instructions` (plus optional `model`).
//!
//! This module is the pure functional core for that translation: it takes the
//! source markdown text and produces the TOML text, with no I/O. The mapping is
//! intentionally simple and lossless for the fields codex understands:
//!
//! | codex field              | source                                   |
//! |--------------------------|------------------------------------------|
//! | `name`                   | frontmatter `name`, else the artifact name |
//! | `description`            | frontmatter `description` (empty if absent) |
//! | `model`                  | frontmatter `model` (omitted if absent)  |
//! | `developer_instructions` | the markdown body (everything after frontmatter) |
//!
//! Strings are emitted as single-line TOML basic strings with full escaping, so
//! arbitrary markdown bodies (including `"""`, quotes, and backslashes) round
//! trip without relying on multi-line string edge cases.

use crate::scan::extract_field;

/// Convert a cmx markdown agent document into codex subagent TOML.
///
/// `markdown` is the full source file content; `fallback_name` is used for the
/// `name` field when the frontmatter omits one (typically the artifact's file
/// stem).
pub fn markdown_to_codex_toml(markdown: &str, fallback_name: &str) -> String {
    let (frontmatter, body) = split_frontmatter_and_body(markdown);

    let name = frontmatter
        .as_deref()
        .and_then(|fm| extract_field(fm, "name"))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    let description = frontmatter
        .as_deref()
        .and_then(|fm| extract_field(fm, "description"))
        .unwrap_or_default();
    let model = frontmatter.as_deref().and_then(|fm| extract_field(fm, "model"));

    let mut out = String::new();
    out.push_str(&toml_kv("name", &name));
    out.push_str(&toml_kv("description", &description));
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        out.push_str(&toml_kv("model", &model));
    }
    out.push_str(&toml_kv("developer_instructions", body.trim_end_matches('\n')));
    out
}

/// Split markdown into its YAML frontmatter (without the `---` fences) and the
/// remaining body. Returns `(None, full_content)` when there is no frontmatter.
fn split_frontmatter_and_body(content: &str) -> (Option<String>, &str) {
    // Frontmatter must start at the very beginning of the file.
    let Some(rest) = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n"))
    else {
        return (None, content);
    };

    // Find the closing fence: a line containing only `---`.
    let mut search_start = 0;
    while let Some(idx) = rest[search_start..].find("---") {
        let abs = search_start + idx;
        let at_line_start = abs == 0 || rest.as_bytes()[abs - 1] == b'\n';
        let after = &rest[abs + 3..];
        let ends_line = after.is_empty() || after.starts_with('\n') || after.starts_with('\r');
        if at_line_start && ends_line {
            let frontmatter = rest[..abs].to_string();
            // Body is everything after the closing fence's line break.
            let body =
                after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
            return (Some(frontmatter), body);
        }
        search_start = abs + 3;
    }

    // Unterminated frontmatter — treat the whole thing as body.
    (None, content)
}

/// Render a single `key = "value"` TOML line with a fully escaped basic string.
fn toml_kv(key: &str, value: &str) -> String {
    format!("{key} = {}\n", toml_basic_string(value))
}

/// Encode a string as a TOML single-line basic string (quoted, with escapes).
///
/// Follows the TOML spec's basic-string escape rules so that any input — quotes,
/// backslashes, newlines, control characters — produces valid TOML.
fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_string_escapes_quotes_and_backslashes() {
        assert_eq!(toml_basic_string(r#"a "b" \ c"#), r#""a \"b\" \\ c""#);
    }

    #[test]
    fn basic_string_escapes_newlines_and_tabs() {
        assert_eq!(toml_basic_string("line1\nline2\tend"), r#""line1\nline2\tend""#);
    }

    #[test]
    fn basic_string_escapes_control_chars_as_unicode() {
        assert_eq!(toml_basic_string("\u{1}"), "\"\\u0001\"");
        assert_eq!(toml_basic_string("\u{7f}"), "\"\\u007F\"");
    }

    #[test]
    fn basic_string_passes_through_unicode_text() {
        assert_eq!(toml_basic_string("café — 日本語"), "\"café — 日本語\"");
    }

    #[test]
    fn triple_quotes_in_body_are_escaped_not_treated_as_multiline() {
        let out = toml_basic_string(r#"use """ here"#);
        assert_eq!(out, r#""use \"\"\" here""#);
    }

    // --- split_frontmatter_and_body ---

    #[test]
    fn split_extracts_frontmatter_and_body() {
        let (fm, body) = split_frontmatter_and_body("---\nname: a\n---\nHello body\n");
        assert_eq!(fm.as_deref(), Some("name: a\n"));
        assert_eq!(body, "Hello body\n");
    }

    #[test]
    fn split_no_frontmatter_returns_full_body() {
        let (fm, body) = split_frontmatter_and_body("Just a body\n");
        assert!(fm.is_none());
        assert_eq!(body, "Just a body\n");
    }

    #[test]
    fn split_unterminated_frontmatter_is_treated_as_body() {
        let content = "---\nname: a\nno closing fence";
        let (fm, body) = split_frontmatter_and_body(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn split_handles_crlf_line_endings() {
        let (fm, body) = split_frontmatter_and_body("---\r\nname: a\r\n---\r\nBody\r\n");
        assert_eq!(fm.as_deref(), Some("name: a\r\n"));
        assert_eq!(body, "Body\r\n");
    }

    // --- markdown_to_codex_toml ---

    #[test]
    fn converts_full_agent_to_toml() {
        let md = "---\nname: rust-craftsperson\ndescription: An expert Rust agent\nmodel: gpt-5\n---\nYou are a meticulous Rust engineer.\n";
        let toml = markdown_to_codex_toml(md, "fallback");
        assert!(toml.contains("name = \"rust-craftsperson\"\n"));
        assert!(toml.contains("description = \"An expert Rust agent\"\n"));
        assert!(toml.contains("model = \"gpt-5\"\n"));
        assert!(
            toml.contains("developer_instructions = \"You are a meticulous Rust engineer.\"\n"),
            "got: {toml}"
        );
    }

    #[test]
    fn uses_fallback_name_when_frontmatter_lacks_name() {
        let md = "---\ndescription: desc\n---\nBody\n";
        let toml = markdown_to_codex_toml(md, "my-agent");
        assert!(toml.contains("name = \"my-agent\"\n"), "got: {toml}");
    }

    #[test]
    fn omits_model_when_absent() {
        let md = "---\nname: a\ndescription: d\n---\nBody\n";
        let toml = markdown_to_codex_toml(md, "a");
        assert!(!toml.contains("model ="), "model should be omitted: {toml}");
    }

    #[test]
    fn multiline_body_is_emitted_as_escaped_single_line_string() {
        let md = "---\nname: a\ndescription: d\n---\nLine one\nLine two\n";
        let toml = markdown_to_codex_toml(md, "a");
        assert!(
            toml.contains("developer_instructions = \"Line one\\nLine two\"\n"),
            "got: {toml}"
        );
    }

    #[test]
    fn agent_with_no_frontmatter_uses_fallback_and_whole_body() {
        let md = "Just instructions, no frontmatter.\n";
        let toml = markdown_to_codex_toml(md, "loose-agent");
        assert!(toml.contains("name = \"loose-agent\"\n"));
        assert!(toml.contains("description = \"\"\n"));
        assert!(
            toml.contains("developer_instructions = \"Just instructions, no frontmatter.\"\n"),
            "got: {toml}"
        );
    }
}
