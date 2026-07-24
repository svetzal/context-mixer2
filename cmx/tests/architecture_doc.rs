//! Architecture documentation drift guard.
//!
//! These tests enforce that `AGENTS.md`'s Architecture section stays
//! synchronized with the actual source tree:
//!
//! - `every_documented_path_exists`: every backtick-quoted path that looks
//!   like a source file (`cmx/src/…`, `cmx-core/src/…`, or `cmf/src/…`)
//!   must exist on disk.
//!
//! - `every_source_file_is_documented`: every `*.rs` file under `cmx/src`,
//!   `cmx-core/src`, and `cmf/src` must appear (by its full repo-relative
//!   path) somewhere in `AGENTS.md`.
//!
//! - `every_documented_module_has_module_docs`: every `*.rs` file under
//!   `cmx/src` and `cmf/src` (excluding `tests.rs` files) must carry a
//!   `//!` module-level doc comment as its first substantive line. AGENTS.md
//!   is the index; the module's own `//!` header is the authoritative
//!   description of its purpose.
//!
//! When you add, move, or delete a module, update the Architecture section in
//! the same commit. The quality-gate sentence in `AGENTS.md` says so too.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cmx crate must have a parent workspace directory")
        .to_path_buf()
}

fn agents_md_content() -> String {
    let path = workspace_root().join("AGENTS.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read AGENTS.md at {}: {}", path.display(), e))
}

/// Return true if a backtick-quoted token looks like a repo-relative Rust
/// source path for one of the three crates we care about.
fn is_source_path(s: &str) -> bool {
    let has_prefix =
        s.starts_with("cmx/src/") || s.starts_with("cmx-core/src/") || s.starts_with("cmf/src/");
    let has_rs_ext = Path::new(s).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
    has_prefix
        && has_rs_ext
        && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

/// Extract every backtick-quoted token from `content` that matches
/// `is_source_path`.
fn extract_documented_paths(content: &str) -> HashSet<String> {
    let mut paths = HashSet::new();
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            let token: String = chars.by_ref().take_while(|&c| c != '`').collect();
            if is_source_path(&token) {
                paths.insert(token);
            }
        }
    }
    paths
}

/// Recursively collect repo-relative paths of all `*.rs` files under `dir`,
/// using `prefix` as the path prefix (e.g. `"cmx/src"`).
fn walk_source_files(root: &Path, subdir: &str) -> Vec<String> {
    let dir = root.join(subdir);
    let mut files = Vec::new();
    walk_dir(&dir, subdir, &mut files);
    files.sort();
    files
}

fn walk_dir(dir: &Path, prefix: &str, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let rel = format!("{prefix}/{name_str}");
        if path.is_dir() {
            walk_dir(&path, &rel, files);
        } else if Path::new(name_str.as_ref())
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            files.push(rel);
        }
    }
}

#[test]
fn every_documented_path_exists() {
    let root = workspace_root();
    let content = agents_md_content();
    let documented = extract_documented_paths(&content);

    let mut missing: Vec<_> =
        documented.iter().filter(|p| !root.join(p.as_str()).exists()).collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "AGENTS.md documents {} path(s) that do not exist on disk:\n{}\n\n\
         Remove stale bullets or rename them to match the actual file location.",
        missing.len(),
        missing.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
    );
}

/// Return the first non-blank line of `content`, or `None` if the file is
/// entirely blank.
fn first_non_blank_line(content: &str) -> Option<&str> {
    content.lines().find(|line| !line.trim().is_empty())
}

#[test]
fn every_documented_module_has_module_docs() {
    let root = workspace_root();

    let mut all_source_files = Vec::new();
    for subdir in &["cmx/src", "cmf/src"] {
        all_source_files.extend(walk_source_files(&root, subdir));
    }

    let mut undocumented: Vec<String> = Vec::new();
    for rel_path in &all_source_files {
        if Path::new(rel_path).file_name().and_then(|n| n.to_str()) == Some("tests.rs") {
            continue;
        }
        let full_path = root.join(rel_path);
        let content = fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", full_path.display(), e));
        match first_non_blank_line(&content) {
            Some(line) if line.starts_with("//!") => {}
            _ => undocumented.push(rel_path.clone()),
        }
    }
    undocumented.sort();

    assert!(
        undocumented.is_empty(),
        "{} source file(s) are missing a `//!` module-level doc comment as \
         their first non-blank line:\n{}\n\n\
         Add a `//!` header describing the module's purpose (see AGENTS.md's \
         Architecture section for the stated purpose of each file).",
        undocumented.len(),
        undocumented.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn every_source_file_is_documented() {
    let root = workspace_root();
    let content = agents_md_content();

    let mut all_source_files = Vec::new();
    for subdir in &["cmx/src", "cmx-core/src", "cmf/src"] {
        all_source_files.extend(walk_source_files(&root, subdir));
    }

    let mut undocumented: Vec<_> =
        all_source_files.iter().filter(|p| !content.contains(p.as_str())).collect();
    undocumented.sort();

    assert!(
        undocumented.is_empty(),
        "{} source file(s) are not documented in AGENTS.md:\n{}\n\n\
         Add a bullet for each new module in the Architecture section.",
        undocumented.len(),
        undocumented.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
    );
}
