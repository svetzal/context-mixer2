//! Architectural guard: `anyhow` must not appear in cmx's core modules.
//!
//! Only `main.rs`, `dispatch/`, and the bridging `error.rs` module are
//! allowed to use `anyhow` — everything else must use `crate::error::Result`
//! and the typed `CliError` enum.

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

fn is_permitted(path: &Path, src_root: &Path) -> bool {
    let rel = path.strip_prefix(src_root).unwrap_or(path);
    let rel_str = rel.to_string_lossy();
    // Permitted: the dispatch layer and the bridging error module.
    rel_str == "main.rs" || rel_str.starts_with("dispatch/") || rel_str == "error.rs"
}

#[test]
fn no_anyhow_in_core_modules() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_rust_files(&src_root);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        if is_permitted(file, &src_root) {
            continue;
        }

        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));

        // Detect any mention of anyhow (macro use, type path, or import).
        let patterns = [
            "anyhow::",
            "use anyhow",
            "bail!",
            "#[macro_use] extern crate anyhow",
        ];
        for pattern in patterns {
            if content.contains(pattern) {
                let rel = file.strip_prefix(&src_root).unwrap_or(file);
                violations
                    .push(format!("{}: contains forbidden pattern `{pattern}`", rel.display()));
                break; // one violation per file is enough
            }
        }
    }

    assert!(
        violations.is_empty(),
        "anyhow found in cmx core modules (not permitted outside main.rs / dispatch/ / error.rs):\n{}",
        violations.join("\n")
    );
}
