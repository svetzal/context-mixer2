//! Tool-neutral agent document transformations.

use crate::frontmatter::split_frontmatter_spans;

/// Convert a markdown agent into Codex's TOML subagent format.
pub fn markdown_to_codex_toml(markdown: &str, fallback_name: &str) -> String {
    let (frontmatter, body) = split_markdown(markdown);
    let name = field(frontmatter, "name")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    let description = field(frontmatter, "description").unwrap_or_default();
    let model = field(frontmatter, "model").filter(|v| !v.is_empty());

    let mut out = String::new();
    out.push_str(&toml_kv("name", &name));
    out.push_str(&toml_kv("description", &description));
    if let Some(model) = model {
        out.push_str(&toml_kv("model", &model));
    }
    out.push_str(&toml_kv("developer_instructions", body.trim_end_matches('\n')));
    out
}

fn split_markdown(content: &str) -> (Option<&str>, &str) {
    let Some(spans) = split_frontmatter_spans(content) else {
        return (None, content);
    };
    let body = spans
        .closing_and_body
        .strip_prefix("---\r\n")
        .or_else(|| spans.closing_and_body.strip_prefix("---\n"))
        .unwrap_or("");
    (Some(spans.inner), body)
}

fn field(frontmatter: Option<&str>, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter?.lines().find_map(|line| {
        if line.starts_with([' ', '\t']) {
            return None;
        }
        line.strip_prefix(&prefix)
            .map(str::trim)
            .map(|value| value.trim_matches(['\'', '"']).to_string())
    })
}

fn toml_kv(key: &str, value: &str) -> String {
    format!("{key} = {}\n", toml_basic_string(value))
}

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
    fn converts_markdown_to_codex_toml() {
        let markdown = "---\nname: reviewer\ndescription: Reviews code\nmodel: gpt-5\n---\nReview carefully.\n";
        let out = markdown_to_codex_toml(markdown, "fallback");
        assert!(out.contains("name = \"reviewer\""));
        assert!(out.contains("description = \"Reviews code\""));
        assert!(out.contains("model = \"gpt-5\""));
        assert!(out.contains("developer_instructions = \"Review carefully.\""));
    }

    #[test]
    fn escapes_multiline_instructions() {
        let out = markdown_to_codex_toml("one\ntwo\n", "reviewer");
        assert!(out.contains("developer_instructions = \"one\\ntwo\""));
    }
}
