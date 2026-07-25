//! Architectural guard: canonical helpers stay canonical.
//!
//! A handful of small string literals and one hand-rolled function used to be
//! duplicated across `cmx/src` before being collapsed into a single canonical
//! definition each (see `cmx/src/display/util.rs`, `cmx/src/table.rs`). The
//! cautionary tale for *why* this guard exists is a real, shipped bug: `cmx
//! skill promote` / `cmx agent promote` iterated `platform_iter::all()`
//! instead of the configured managed-platform allowlist
//! (`config::managed_or_all_platforms`), unlike every sibling command
//! (`install`/`uninstall`/`sync`/`diff`/`sets`). That drift was only possible
//! because there was no single call path those commands were required to
//! share — each was free to hand-roll its own platform iteration, and one of
//! them hand-rolled it wrong. Re-duplicating a literal or a hand-rolled loop
//! is how that class of bug comes back: a fix or a fallback value can be
//! updated in the canonical spot while a copy-pasted duplicate silently keeps
//! the old (or wrong) behavior.
//!
//! This test walks `cmx/src/**/*.rs` (excluding `tests.rs` files, inline
//! `#[cfg(test)] mod tests` blocks, and the modules that legitimately define
//! each canonical item) and fails if:
//!
//! (a) the `"unversioned"` fallback literal appears outside
//!     `display/util.rs` (its one definition) and `list.rs`'s
//!     `ListStatus::Unversioned => "unversioned"` arm (a status discriminant
//!     label, not a version-fallback decision — see `SETS.md`/list.rs docs
//!     for why that's a different decision);
//! (b) the `"Re-run with --apply to make these changes."` literal appears
//!     outside `display/util.rs` (its one definition, `APPLY_HINT`);
//! (c) `platform_iter::all()` appears in any of the cross-platform command
//!     modules that must instead resolve platforms via
//!     `config::managed_or_all_platforms` — `promote.rs`, `install.rs`,
//!     `uninstall.rs`, `sync.rs`, `diff/`, `sets/`;
//! (d) a second `fn write_discarded_paths` is declared anywhere but
//!     `display/util.rs`.
//!
//! To retire one of these checks, first satisfy yourself the underlying
//! duplication has been re-justified as a genuinely different decision (as
//! `list.rs`'s `ListStatus` arm and `section_str`'s `"  (none)\n"` are), not
//! just re-introduced for convenience.

use std::path::{Path, PathBuf};

fn collect_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_rust_files(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files
}

/// Strip inline `#[cfg(test)] mod tests { ... }` blocks (naive brace counting,
/// good enough for this codebase's formatting) so literals used only in test
/// assertions don't trip the guard — the guard is about production fallback
/// decisions, not about what strings a test happens to assert on.
fn strip_inline_test_mod(content: &str) -> String {
    let Some(marker_pos) = content.find("#[cfg(test)]") else {
        return content.to_string();
    };
    let Some(mod_rel_pos) = content[marker_pos..].find("mod tests") else {
        return content.to_string();
    };
    let mod_pos = marker_pos + mod_rel_pos;
    let Some(brace_rel_pos) = content[mod_pos..].find('{') else {
        return content.to_string();
    };
    let open_pos = mod_pos + brace_rel_pos;

    let mut depth = 0i32;
    let mut end = content.len();
    for (i, ch) in content[open_pos..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open_pos + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    format!("{}{}", &content[..marker_pos], &content[end..])
}

fn production_content(path: &Path) -> Option<String> {
    let rel = path.to_string_lossy();
    if rel.ends_with("tests.rs") {
        return None;
    }
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    Some(strip_inline_test_mod(&content))
}

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn unversioned_literal_stays_in_its_canonical_spot() {
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        let is_definition = rel == "display/util.rs";
        // `Self::Unversioned => "unversioned"` in `ListStatus::label` — a status
        // discriminant label, not a version-fallback decision.
        let is_list_status_arm = rel == "list.rs";
        for (i, line) in content.lines().enumerate() {
            if !line.contains("\"unversioned\"") {
                continue;
            }
            if is_definition {
                continue;
            }
            if is_list_status_arm && line.contains("Self::Unversioned") {
                continue;
            }
            violations.push(format!(
                "{rel}:{}: \"unversioned\" literal outside display/util.rs::version_label — call \
                 that instead",
                i + 1
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn apply_hint_literal_stays_in_its_canonical_spot() {
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        if rel == "display/util.rs" {
            continue;
        }
        for (i, line) in content.lines().enumerate() {
            if line.contains("Re-run with --apply to make these changes.") {
                violations.push(format!(
                    "{rel}:{}: apply-hint literal outside display/util.rs::APPLY_HINT — use that \
                     constant instead",
                    i + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn cross_platform_commands_never_bypass_the_managed_platform_allowlist() {
    // These modules front commands whose whole point is to act only on the
    // platforms the user configured via `cmx config platforms` (or, absent a
    // config, every supported platform). `platform_iter::all()` skips that
    // allowlist entirely — that's exactly the bug this guard exists to catch.
    let guarded_paths = [
        "promote.rs",
        "install.rs",
        "uninstall.rs",
        "sync.rs",
        "diff/",
        "sets/",
    ];
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        if !guarded_paths.iter().any(|p| rel == *p || rel.starts_with(p)) {
            continue;
        }
        for (i, line) in content.lines().enumerate() {
            if line.contains("platform_iter::all()") {
                violations.push(format!(
                    "{rel}:{}: platform_iter::all() bypasses the managed-platform allowlist — \
                     use config::managed_or_all_platforms instead",
                    i + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn write_discarded_paths_has_exactly_one_definition() {
    let root = src_root();
    let mut definitions = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        for (i, line) in content.lines().enumerate() {
            if line.contains("fn write_discarded_paths") {
                definitions.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    assert_eq!(
        definitions.len(),
        1,
        "expected exactly one fn write_discarded_paths (in display/util.rs), found: {definitions:?}"
    );
    assert!(
        definitions[0].starts_with("display/util.rs"),
        "the one definition should live in display/util.rs, found: {definitions:?}"
    );
}
