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
//! A second cautionary tale, same root cause: `adopt.rs`'s `unadopt_one` used
//! to hand-roll its own "which platforms track this artifact from home"
//! lookup, and — independently from the `promote` bug above — iterated every
//! supported platform (`platform_iter::all()`) instead of the managed
//! allowlist too. It also disagreed on error handling with the other two
//! hand-rolled copies of that same lookup (`promote.rs`, `sync.rs`): one used
//! `.ok()?` to silently skip a platform whose lock file failed to parse, the
//! others propagated with `?`. Both problems went away once the lookup moved
//! into `cmx/src/home_provenance.rs` as the one definition every caller shares
//! — see that module's header for the full account.
//!
//! A third recurrence, same root cause again: `doctor/survey.rs` inlined its
//! own `if cfg.platforms.is_empty() { Platform::ALL.to_vec() } else { ... }`
//! instead of calling `config::managed_or_all_platforms`, and both
//! `suggestions.rs` and `info/mod.rs` iterated `Platform::ALL` directly with
//! no allowlist check at all. None of these were caught by this guard at the
//! time, because the guard only scanned a *path-scoped allowlist*
//! (`guarded_paths`, a fixed list of module paths) and only matched the
//! literal string `platform_iter::all()` — a new module bypassing the
//! allowlist a different way (inlining the fallback, or iterating
//! `Platform::ALL` instead of calling `platform_iter::all()`) could always
//! slip through both restrictions. The guard now scans every production file
//! under `cmx/src` and matches both `platform_iter::all()` and
//! `Platform::ALL`, so there is no module path and no spelling left for this
//! bug to hide behind.
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
//! (c) `platform_iter::all()` or `Platform::ALL` appears anywhere under
//!     `cmx/src` outside the modules that legitimately define platform
//!     enumeration itself — every cross-platform command must instead resolve
//!     platforms via `config::managed_or_all_platforms` (or, for an
//!     active-platform-first search, `platform_iter::active_first_of` fed by
//!     that same call);
//! (d) a second `fn write_discarded_paths` is declared anywhere but
//!     `display/util.rs`;
//! (e) the home-provenance check (`== HOME_SOURCE` or `HOME_SOURCE ==`)
//!     appears anywhere but `home_provenance.rs`;
//! (f) `.is_file()` (the zero-arg inherent `Path::is_file` call) appears
//!     anywhere under `cmx/src` — every path-kind check must go through the
//!     `Filesystem` gateway (`fs.is_file(path)`) instead;
//! (g) a second `fn representative_platform` is declared anywhere but
//!     `platform_copies.rs`;
//! (h) a second `fn changed_target_path` is declared anywhere but `diff/mod.rs`.
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
fn platform_enumeration_always_goes_through_the_managed_allowlist() {
    // Every command that acts across platforms must resolve its candidate set
    // via `config::managed_or_all_platforms` (the platforms the user
    // configured via `cmx config platforms`, or every supported platform
    // absent a config) — never by iterating `platform_iter::all()` or
    // `Platform::ALL` directly, which silently bypasses that allowlist.
    //
    // This used to be a *path-scoped* allowlist of "guarded" command modules,
    // matching only the literal `platform_iter::all()`. That was itself the
    // defect: a new module could always bypass cross-platform scoping outside
    // the listed paths, or by iterating `Platform::ALL` directly instead of
    // calling `platform_iter::all()` — both of which happened (see the module
    // header). So this guard now scans every production file under `cmx/src`
    // and matches both spellings, with no path list left to fall outside of.
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        for (i, line) in content.lines().enumerate() {
            if line.contains("platform_iter::all()") || line.contains("Platform::ALL") {
                violations.push(format!(
                    "{rel}:{}: bypasses the managed-platform allowlist — use \
                     config::managed_or_all_platforms instead",
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

#[test]
fn home_provenance_check_has_one_implementation() {
    // "which platforms track this artifact from the canonical home" is
    // answered by comparing a lock entry's `source.repo` against
    // `HOME_SOURCE`. That comparison must live only in home_provenance.rs —
    // see its module header for the bug this guard exists to catch.
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        if rel == "home_provenance.rs" {
            continue;
        }
        for (i, line) in content.lines().enumerate() {
            if line.contains("== HOME_SOURCE") || line.contains("HOME_SOURCE ==") {
                violations.push(format!(
                    "{rel}:{}: hand-rolled home-provenance check outside home_provenance.rs — use \
                     home_provenance::home_tracked_entries instead",
                    i + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn path_is_file_is_never_called_directly() {
    // Every path-kind check must go through the `Filesystem` gateway
    // (`fs.is_file(path)`), never `std::path::Path::is_file()` directly — the
    // gateway is the only path-kind oracle in this codebase's
    // functional-core/imperative-shell architecture.
    let root = src_root();
    let mut violations = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        for (i, line) in content.lines().enumerate() {
            if line.contains(".is_file()") {
                violations.push(format!(
                    "{rel}:{}: bare Path::is_file() call — use fs.is_file(path) via the \
                     Filesystem gateway instead",
                    i + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn representative_platform_has_exactly_one_definition() {
    let root = src_root();
    let mut definitions = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        for (i, line) in content.lines().enumerate() {
            if line.contains("fn representative_platform") {
                definitions.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    assert_eq!(
        definitions.len(),
        1,
        "expected exactly one fn representative_platform (in platform_copies.rs), found: \
         {definitions:?}"
    );
    assert!(
        definitions[0].starts_with("platform_copies.rs"),
        "the one definition should live in platform_copies.rs, found: {definitions:?}"
    );
}

#[test]
fn changed_target_path_has_exactly_one_definition() {
    let root = src_root();
    let mut definitions = Vec::new();
    for file in collect_rust_files(&root) {
        let Some(content) = production_content(&file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(&file).to_string_lossy().to_string();
        for (i, line) in content.lines().enumerate() {
            if line.contains("fn changed_target_path") {
                definitions.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    assert_eq!(
        definitions.len(),
        1,
        "expected exactly one fn changed_target_path (in diff/mod.rs), found: {definitions:?}"
    );
    assert!(
        definitions[0].starts_with("diff/mod.rs"),
        "the one definition should live in diff/mod.rs, found: {definitions:?}"
    );
}
