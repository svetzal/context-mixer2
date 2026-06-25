use serde_yaml_ng::{Mapping, Value};

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
pub(crate) fn normalize_frontmatter(frontmatter: &str) -> String {
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
pub(crate) fn fix_unquoted_block_indicator_value(line: &str) -> String {
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
pub(crate) fn lenient_mapping(frontmatter: &str) -> Option<Mapping> {
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
pub(crate) fn scalar_to_string(value: &Value) -> Option<String> {
    let s = match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let trimmed = s.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_recovers_multiparagraph_description() {
        let text = "name: personal-finance\n\
                    description: First paragraph runs long.\n\
                    \n\
                    Second paragraph after a blank line.";
        let mapping = lenient_mapping(text).expect("fallback should recover a mapping");
        assert!(mapping.contains_key(Value::String("name".to_string())));
        let desc = mapping.get(Value::String("description".to_string())).unwrap();
        if let Value::String(s) = desc {
            assert!(
                s.contains("First paragraph") && s.contains("Second paragraph"),
                "paragraphs joined: {s}"
            );
        } else {
            panic!("description should be a String value");
        }
        let name = mapping.get(Value::String("name".to_string())).unwrap();
        assert_eq!(scalar_to_string(name).as_deref(), Some("personal-finance"));
    }

    #[test]
    fn normalize_frontmatter_replaces_leading_tabs() {
        let input = "metadata:\n\tversion: 1.0.0\n";
        let out = normalize_frontmatter(input);
        assert!(!out.contains('\t'), "tabs replaced: {out}");
        assert!(out.contains("  version: 1.0.0"), "two-space indent: {out}");
    }

    #[test]
    fn fix_unquoted_block_indicator_quotes_gt_inline_value() {
        let line = "description: >= 2.0 required";
        let out = fix_unquoted_block_indicator_value(line);
        assert!(out.contains("'>= 2.0 required'"), "quoted: {out}");
    }

    #[test]
    fn fix_unquoted_block_indicator_leaves_real_block_scalar_header() {
        // `>` alone (or `>-`) is a valid block-scalar header — don't touch it.
        let line = "description: >";
        let out = fix_unquoted_block_indicator_value(line);
        assert_eq!(out, line);
    }

    #[test]
    fn scalar_to_string_returns_string_value() {
        let v = Value::String("hello".to_string());
        assert_eq!(scalar_to_string(&v), Some("hello".to_string()));
    }

    #[test]
    fn scalar_to_string_returns_bool_as_string() {
        assert_eq!(scalar_to_string(&Value::Bool(true)), Some("true".to_string()));
    }

    #[test]
    fn scalar_to_string_returns_none_for_null() {
        assert_eq!(scalar_to_string(&Value::Null), None);
    }

    #[test]
    fn scalar_to_string_returns_none_for_empty_string() {
        assert_eq!(scalar_to_string(&Value::String("   ".to_string())), None);
    }
}
