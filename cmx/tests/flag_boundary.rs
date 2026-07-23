//! Architectural guard: no "boolean blindness" at internal call sites.
//!
//! `force`, `purge`, `apply`, `local`, and `include_local` are exactly the raw
//! `bool` names clap hands us for `--force`, `--purge`, `--apply`, `--local`,
//! and the survey-breadth flag. Once past the dispatch boundary they must be
//! wrapped in an intent-revealing enum (`Force`, `Purge`, `RunMode`,
//! `InstallScope`, `SurveyScope` — see `cmx/src/flags.rs`) rather than passed
//! on as a bare `bool` **function parameter**. A bare-bool parameter defeats
//! the purpose of having those enums: it forces every reader of the callee to
//! go find the call site to learn what `true` means.
//!
//! This test walks `cmx/src` (skipping `cmx/src/cli/`, whose clap-derived
//! structs legitimately declare raw bool fields parsed straight from argv) and
//! fails if it finds one of those five names declared as `bool` in a function
//! parameter position.
//!
//! Two shapes are intentionally allowed and excluded from the scan:
//!
//! - `pub <name>: bool` struct fields — serialized report/display data (e.g.
//!   `SyncResult.apply`, `PromoteResult.apply`, `AdoptOutcome.included_local`),
//!   not parameters threaded through core logic.
//! - The `from_flag`/`scope_from` conversion constructors themselves (in
//!   `cmx/src/flags.rs` and `cmx/src/dispatch/mod.rs`) — the one deliberate
//!   place each raw bool is unwrapped into its enum, right at the dispatch
//!   boundary.
//!
//! To add a new legitimate bare-bool parameter, extend the `is_permitted`
//! allowlist below with a short comment explaining why it's exempt — don't
//! just widen the `FLAG_NAMES` list or the `is_report_field`/`is_converter`
//! checks, since that would quietly reopen the hole this test exists to keep
//! shut.

use std::path::{Path, PathBuf};

/// Bare-bool names that must not appear as a function parameter outside the
/// dispatch boundary — each has a corresponding intent-revealing enum in
/// `cmx/src/flags.rs` (or, for `local`/`include_local`, `InstallScope`/
/// `SurveyScope`).
const FLAG_NAMES: &[&str] = &["force", "purge", "apply", "local", "include_local"];

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

fn is_permitted_file(path: &Path, src_root: &Path) -> bool {
    let rel = path.strip_prefix(src_root).unwrap_or(path);
    let rel_str = rel.to_string_lossy();
    // clap-derived CLI structs legitimately declare raw bool fields parsed
    // straight from argv.
    rel_str.starts_with("cli/")
}

/// `true` for a `pub <name>: bool` struct field — legitimate serialized
/// report data, not a function parameter.
fn is_report_field(line: &str) -> bool {
    line.trim_start().starts_with("pub ")
}

/// `true` for the one deliberate conversion point (`from_flag`/`scope_from`)
/// that unwraps a raw bool into its enum.
fn is_converter(line: &str) -> bool {
    line.contains("fn from_flag(") || line.contains("fn scope_from(")
}

#[test]
fn no_bare_bool_flag_parameters_outside_dispatch_boundary() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_rust_files(&src_root);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        if is_permitted_file(file, &src_root) {
            continue;
        }

        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));

        for (i, line) in content.lines().enumerate() {
            if is_report_field(line) || is_converter(line) {
                continue;
            }
            for name in FLAG_NAMES {
                let pattern = format!("{name}: bool");
                if line.contains(&pattern) {
                    let rel = file.strip_prefix(&src_root).unwrap_or(file);
                    violations.push(format!(
                        "{}:{}: bare `{pattern}` parameter — wrap it in its intent-revealing enum",
                        rel.display(),
                        i + 1,
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "boolean-blind parameters found (not permitted outside cli/, report struct fields, \
         or the from_flag/scope_from converters):\n{}",
        violations.join("\n")
    );
}
