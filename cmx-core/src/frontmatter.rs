//! Canonical skill-version reconciliation.
//!
//! Embedding tools declare their version exactly once — via
//! [`ToolIdentity`](crate::skill_install::ToolIdentity). cmx-core then guarantees
//! the installed `SKILL.md` carries that same version in its frontmatter, so the
//! version tracked in the lockfile and the version a reader (`cmx doctor`,
//! `cmx list`, or any agent-skills-aware tool) parses from the file always agree.
//!
//! The reconciled key is `metadata.version` — the agent-skills community
//! standard. This module owns the single, tested implementation so embedders no
//! longer hand-roll their own (divergent, and historically wrong-keyed) frontmatter
//! stampers.
//!
//! ## What reconciliation does
//!
//! Given the bundled `SKILL.md`, it produces content where the frontmatter's
//! `metadata.version` equals the tool version:
//!
//! - `metadata.version` present → its value is replaced (indentation and key
//!   preserved).
//! - `metadata:` block present without a `version:` → a `version:` child is added.
//! - No `metadata:` block → a `metadata:` block with `version:` is appended.
//! - A shadowing top-level `version:` (which the community reader consults before
//!   `metadata.version`) is removed, so the reconciled `metadata.version` wins.
//!
//! Editing is surgical (line-preserving), never a YAML re-emit: human-authored
//! frontmatter — folded description blocks, comments, key order — is left byte-for-byte
//! intact apart from the single version line. The transform is deterministic and
//! idempotent, so it composes cleanly with cmx-core's checksum-based skip/drift
//! guards.

use crate::skill_fs::SkillFile;

/// Return a copy of `files` in which the root `SKILL.md`'s frontmatter carries
/// `metadata.version = version`. All other files are returned unchanged.
///
/// A `SKILL.md` without a leading `---` frontmatter fence is left untouched (there
/// is nothing to reconcile into).
pub fn reconcile_skill_version(files: &[SkillFile], version: &str) -> Vec<SkillFile> {
    files
        .iter()
        .map(|f| {
            if f.rel_path.as_os_str() == "SKILL.md"
                && let Ok(text) = std::str::from_utf8(&f.bytes)
            {
                let reconciled = set_metadata_version(text, version);
                return SkillFile {
                    rel_path: f.rel_path.clone(),
                    bytes: reconciled.into_bytes(),
                };
            }
            f.clone()
        })
        .collect()
}

/// Set `metadata.version` in `content`'s leading YAML frontmatter to `version`,
/// preserving all other bytes. Returns `content` unchanged when it has no
/// frontmatter fence.
fn set_metadata_version(content: &str, version: &str) -> String {
    let value = format!("\"{version}\"");

    // The opening fence must be the first line.
    let (open, after_open) = if let Some(rest) = content.strip_prefix("---\n") {
        ("---\n", rest)
    } else if let Some(rest) = content.strip_prefix("---\r\n") {
        ("---\r\n", rest)
    } else {
        return content.to_string();
    };

    // Find the closing fence: a line that is exactly `---` (ignoring a trailing \r).
    let Some(fence_start) = find_closing_fence(after_open) else {
        return content.to_string();
    };

    let inner = &after_open[..fence_start];
    let closing_and_rest = &after_open[fence_start..];
    let new_inner = reconcile_inner(inner, &value);

    let mut out = String::with_capacity(content.len() + value.len() + 16);
    out.push_str(open);
    out.push_str(&new_inner);
    out.push_str(closing_and_rest);
    out
}

/// Byte offset within `after_open` of the start of the closing `---` line, or
/// `None` if there is no closing fence.
fn find_closing_fence(after_open: &str) -> Option<usize> {
    let mut line_start = 0;
    loop {
        let rest = &after_open[line_start..];
        let (line, has_newline) = match rest.find('\n') {
            Some(p) => (&rest[..p], true),
            None => (rest, false),
        };
        if line.trim_end_matches('\r') == "---" {
            return Some(line_start);
        }
        if !has_newline {
            return None;
        }
        line_start += line.len() + 1;
    }
}

/// Reconcile the frontmatter inner text (between the fences). Each line in `inner`
/// retains its trailing `\n`; `value` is the already-quoted version literal.
fn reconcile_inner(inner: &str, value: &str) -> String {
    let mut lines: Vec<String> = inner.split_inclusive('\n').map(str::to_string).collect();

    // Drop any top-level `version:` — the community reader consults it before
    // `metadata.version`, so leaving it would shadow the value we set below.
    lines.retain(|l| !is_top_level_key(l, "version"));

    if let Some(meta_idx) = lines.iter().position(|l| is_top_level_key(l, "metadata")) {
        set_version_in_metadata_block(&mut lines, meta_idx, value);
    } else {
        ensure_trailing_newline(&mut lines);
        lines.push("metadata:\n".to_string());
        lines.push(format!("  version: {value}\n"));
    }

    lines.concat()
}

/// Within an existing `metadata:` block (starting at `meta_idx`), set or insert
/// the `version:` child.
fn set_version_in_metadata_block(lines: &mut Vec<String>, meta_idx: usize, value: &str) {
    let mut first_child_indent: Option<String> = None;
    let mut version_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate().skip(meta_idx + 1) {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim().is_empty() {
            continue; // blank line — still inside the block
        }
        let indent = leading_ws(trimmed);
        if indent.is_empty() {
            break; // dedent to top level ends the block
        }
        if first_child_indent.is_none() {
            first_child_indent = Some(indent.to_string());
        }
        if trimmed.trim_start().starts_with("version:") {
            version_idx = Some(i);
            break;
        }
    }

    if let Some(vi) = version_idx {
        let indent = leading_ws(&lines[vi]).to_string();
        lines[vi] = format!("{indent}version: {value}\n");
    } else {
        let indent = first_child_indent.unwrap_or_else(|| "  ".to_string());
        lines.insert(meta_idx + 1, format!("{indent}version: {value}\n"));
    }
}

/// Whether `line` declares the given top-level (unindented) mapping key, e.g.
/// `is_top_level_key("version: 1\n", "version")`.
fn is_top_level_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    trimmed.starts_with(key)
        && trimmed[key.len()..].starts_with(':')
        && !line.starts_with([' ', '\t'])
}

/// The leading whitespace (spaces/tabs) of `s`.
fn leading_ws(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches([' ', '\t']).len()]
}

/// Ensure the last line ends with a newline, so an appended block starts cleanly.
fn ensure_trailing_newline(lines: &mut [String]) {
    if let Some(last) = lines.last_mut()
        && !last.ends_with('\n')
    {
        last.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_via_reader(content: &str) -> Option<String> {
        // Mirror the community reader: top-level `version:` first, then
        // `metadata.version`. Kept intentionally simple — it exists only to assert
        // what a real agent-skills reader would resolve.
        let fm = content.strip_prefix("---\n")?;
        let end = fm.find("\n---")?;
        let fm = &fm[..end];
        let mut in_metadata = false;
        for line in fm.lines() {
            if !line.starts_with([' ', '\t']) {
                in_metadata = line.trim_end() == "metadata:";
                if let Some(rest) = line.strip_prefix("version:") {
                    return Some(unquote(rest.trim()));
                }
            } else if in_metadata && let Some(rest) = line.trim_start().strip_prefix("version:") {
                return Some(unquote(rest.trim()));
            }
        }
        None
    }

    fn unquote(s: &str) -> String {
        s.trim_matches('"').to_string()
    }

    #[test]
    fn replaces_existing_metadata_version() {
        let src = "---\nname: foo\nmetadata:\n  version: \"0.0.0\"\n---\n# Body\n";
        let out = set_metadata_version(src, "1.2.3");
        assert_eq!(version_via_reader(&out).as_deref(), Some("1.2.3"));
        assert!(out.contains("name: foo"), "other keys preserved");
        assert!(out.contains("# Body"), "body preserved");
    }

    #[test]
    fn adds_version_to_metadata_block_without_one() {
        let src = "---\nname: foo\nmetadata:\n  author: me\n---\n# Body\n";
        let out = set_metadata_version(src, "2.0.0");
        assert_eq!(version_via_reader(&out).as_deref(), Some("2.0.0"));
        assert!(out.contains("author: me"), "sibling metadata key preserved");
    }

    #[test]
    fn appends_metadata_block_when_absent() {
        let src = "---\nname: foo\ndescription: bar\n---\n# Body\n";
        let out = set_metadata_version(src, "3.1.0");
        assert_eq!(version_via_reader(&out).as_deref(), Some("3.1.0"));
        assert!(out.contains("metadata:\n  version: \"3.1.0\""));
        assert!(out.contains("description: bar"));
    }

    #[test]
    fn removes_shadowing_top_level_version() {
        // A stale top-level version would win in the reader; it must be dropped so
        // the reconciled metadata.version is what resolves.
        let src = "---\nname: foo\nversion: 9.9.9\nmetadata:\n  author: me\n---\n# Body\n";
        let out = set_metadata_version(src, "1.0.0");
        assert!(!out.contains("version: 9.9.9"), "top-level version removed");
        assert_eq!(version_via_reader(&out).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn preserves_folded_description_block() {
        let src = "---\nname: foo\ndescription: >\n  line one\n  line two\nmetadata:\n  version: \"0.0.0\"\n---\n# Body\n";
        let out = set_metadata_version(src, "4.5.6");
        assert!(out.contains("description: >\n  line one\n  line two"), "folded block intact");
        assert_eq!(version_via_reader(&out).as_deref(), Some("4.5.6"));
    }

    #[test]
    fn preserves_comment_before_version() {
        let src = "---\nmetadata:\n  # placeholder\n  version: \"0.0.0\"\n---\n# Body\n";
        let out = set_metadata_version(src, "1.1.1");
        assert!(out.contains("# placeholder"), "comment preserved");
        assert_eq!(version_via_reader(&out).as_deref(), Some("1.1.1"));
    }

    #[test]
    fn idempotent_second_pass_is_a_no_op() {
        let src = "---\nname: foo\ndescription: bar\n---\n# Body\n";
        let once = set_metadata_version(src, "3.1.0");
        let twice = set_metadata_version(&once, "3.1.0");
        assert_eq!(once, twice, "stamping an already-stamped skill must not change it");
    }

    #[test]
    fn no_frontmatter_returns_unchanged() {
        let src = "# Just a heading\n\nNo frontmatter here.\n";
        assert_eq!(set_metadata_version(src, "1.0.0"), src);
    }

    #[test]
    fn unterminated_frontmatter_returns_unchanged() {
        let src = "---\nname: foo\n# never closed\n";
        assert_eq!(set_metadata_version(src, "1.0.0"), src);
    }

    #[test]
    fn does_not_match_versionish_keys() {
        // `versioning:` must not be treated as the top-level `version:` key.
        let src = "---\nversioning: semver\ndescription: bar\n---\n# Body\n";
        let out = set_metadata_version(src, "1.0.0");
        assert!(out.contains("versioning: semver"), "look-alike key preserved");
        assert_eq!(version_via_reader(&out).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn crlf_frontmatter_is_reconciled() {
        let src = "---\r\nname: foo\r\nmetadata:\r\n  version: \"0.0.0\"\r\n---\r\n# Body\r\n";
        let out = set_metadata_version(src, "7.7.7");
        assert!(out.contains("version: \"7.7.7\""));
    }

    #[test]
    fn reconcile_skill_version_only_touches_skill_md() {
        let files = vec![
            SkillFile::text("SKILL.md", "---\nname: foo\n---\n# Body\n"),
            SkillFile::text("references/x.md", "# untouched version: 1\n"),
        ];
        let out = reconcile_skill_version(&files, "2.2.2");
        let skill = std::str::from_utf8(&out[0].bytes).unwrap();
        assert_eq!(version_via_reader(skill).as_deref(), Some("2.2.2"));
        assert_eq!(out[1].bytes, files[1].bytes, "non-SKILL.md file unchanged");
    }
}
