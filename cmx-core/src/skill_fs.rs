//! In-memory skill file representation and filesystem helpers.
//!
//! A [`BundledSkill`]'s files are carried in memory as [`SkillFile`] values.
//! [`canonical_files`] filters and sorts them the same way `checksum_dir` does,
//! so checksums computed from memory match checksums computed from disk.

use crate::error::Result;
use std::path::{Path, PathBuf};

use crate::checksum::checksum_in_memory;
use crate::fs_util::is_transient;
use crate::gateway::Filesystem;

/// A single file bundled inside a skill.
#[derive(Debug, Clone)]
pub struct SkillFile {
    /// Path relative to the skill's root directory (e.g. `SKILL.md`, `scripts/tool.py`).
    pub rel_path: PathBuf,
    /// Raw file bytes.
    pub bytes: Vec<u8>,
}

impl SkillFile {
    /// Construct from text content, the common case for `include_str!` embeds:
    ///
    /// ```
    /// # use cmx_core::skill_fs::SkillFile;
    /// let f = SkillFile::text("references/workflows.md", "# Workflows\n");
    /// assert_eq!(f.rel_path.to_str(), Some("references/workflows.md"));
    /// ```
    pub fn text(rel_path: impl Into<PathBuf>, content: &str) -> Self {
        Self {
            rel_path: rel_path.into(),
            bytes: content.as_bytes().to_vec(),
        }
    }
}

/// Filter and sort `files` the same way `checksum_dir` processes a directory:
///
/// - Exclude files whose relative path contains a dotfile component (any
///   component that starts with `'.'`).
/// - Exclude files whose relative path contains a transient component
///   (matched by [`is_transient`]).
/// - Sort by the `/`-joined relative-path string for determinism.
///
/// The ordering keys on [`crate::checksum::rel_path_key`] — the same string
/// `checksum_dir` uses — so an in-memory bundle checksum matches the on-disk
/// checksum after [`write_skill_files`], including at the `.`-vs-`/` boundary
/// (SPEC §5.1 / §11.4).
pub fn canonical_files(files: &[SkillFile]) -> Vec<&SkillFile> {
    let mut out: Vec<&SkillFile> = files
        .iter()
        .filter(|f| {
            !f.rel_path.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s.starts_with('.') || is_transient(&s)
            })
        })
        .collect();
    out.sort_by(|a, b| {
        crate::checksum::rel_path_key(&a.rel_path).cmp(&crate::checksum::rel_path_key(&b.rel_path))
    });
    out
}

/// Compute a checksum over the canonical (filtered, sorted) subset of `files`.
///
/// The result matches what `checksum_dir` would produce after [`write_skill_files`]
/// writes the same files to disk.
pub fn checksum_bundled(files: &[SkillFile]) -> String {
    let canonical = canonical_files(files);
    checksum_in_memory(canonical.iter().map(|f| (f.rel_path.as_path(), f.bytes.as_slice())))
}

/// Write every file in `files` into `dest_dir`, creating parent directories as
/// needed.
///
/// All files (including dotfiles and transient) are written — this mirrors what
/// a normal directory copy does. [`canonical_files`] and [`checksum_bundled`]
/// then exclude them from checksums, keeping parity with `checksum_dir`.
pub fn write_skill_files(dest_dir: &Path, files: &[SkillFile], fs: &dyn Filesystem) -> Result<()> {
    fs.create_dir_all(dest_dir)?;
    for file in files {
        let dest = dest_dir.join(&file.rel_path);
        if let Some(parent) = dest.parent() {
            fs.create_dir_all(parent)?;
        }
        fs.write_bytes(&dest, &file.bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;

    fn make_file(rel: &str, bytes: &[u8]) -> SkillFile {
        SkillFile {
            rel_path: PathBuf::from(rel),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn canonical_files_excludes_dotfiles() {
        let files = vec![
            make_file("SKILL.md", b"content"),
            make_file(".hidden", b"hidden"),
            make_file("scripts/.env", b"env"),
        ];
        let canonical = canonical_files(&files);
        let names: Vec<_> = canonical.iter().map(|f| f.rel_path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["SKILL.md"]);
    }

    #[test]
    fn canonical_files_excludes_transient_dirs() {
        let files = vec![
            make_file("SKILL.md", b"content"),
            make_file("node_modules/dep/index.js", b"vendor"),
            make_file("__pycache__/tool.pyc", b"bytecode"),
            make_file("scripts/tool.py", b"code"),
        ];
        let canonical = canonical_files(&files);
        let names: Vec<_> = canonical.iter().map(|f| f.rel_path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["SKILL.md", "scripts/tool.py"]);
    }

    #[test]
    fn canonical_files_sorted_by_rel_path() {
        let files = vec![
            make_file("z.md", b"z"),
            make_file("SKILL.md", b"skill"),
            make_file("a.md", b"a"),
        ];
        let canonical = canonical_files(&files);
        let names: Vec<_> = canonical.iter().map(|f| f.rel_path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["SKILL.md", "a.md", "z.md"]);
    }

    #[test]
    fn write_skill_files_creates_nested_dirs() {
        let fs = FakeFilesystem::new();
        let files = vec![
            make_file("SKILL.md", b"# skill"),
            make_file("scripts/tool.py", b"code"),
        ];
        write_skill_files(Path::new("/dest/my-skill"), &files, &fs).unwrap();
        assert!(fs.file_exists(Path::new("/dest/my-skill/SKILL.md")));
        assert!(fs.file_exists(Path::new("/dest/my-skill/scripts/tool.py")));
    }

    #[test]
    fn checksum_bundled_matches_after_write() {
        let fs = FakeFilesystem::new();
        let files = vec![
            make_file("SKILL.md", b"# skill"),
            make_file("scripts/tool.py", b"code"),
        ];
        let expected = checksum_bundled(&files);
        write_skill_files(Path::new("/dest/skill"), &files, &fs).unwrap();
        let on_disk = crate::checksum::checksum_dir(Path::new("/dest/skill"), &fs).unwrap();
        assert_eq!(expected, on_disk);
    }

    #[test]
    fn canonical_files_string_sort_orders_dot_before_slash() {
        // SPEC §11.4: order by the `/`-joined string, not component-wise. Since
        // '.' (0x2E) < '/' (0x2F), `a.b` sorts before `a/b`; the bare prefix `a`
        // sorts before both. Component-wise `Path` ordering would put `a/b`
        // before `a.b`, so this test fails against the old sort.
        let files = vec![
            make_file("a/b", b"nested"),
            make_file("a.b", b"dotted"),
            make_file("a", b"bare"),
        ];
        let canonical = canonical_files(&files);
        let names: Vec<_> = canonical.iter().map(|f| f.rel_path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["a", "a.b", "a/b"]);
    }

    #[test]
    fn checksum_bundled_matches_after_write_with_dot_slash_paths() {
        // The in-memory and on-disk checksums must still agree once the sort key
        // spans the `.`-vs-`/` boundary — the parity the shared rel_path_key
        // guarantees. (No bare `a` here: it would collide with the `a/` dir on
        // write.)
        let fs = FakeFilesystem::new();
        let files = vec![
            make_file("a/b", b"nested"),
            make_file("a.b", b"dotted"),
            make_file("SKILL.md", b"# skill"),
        ];
        let expected = checksum_bundled(&files);
        write_skill_files(Path::new("/dest/skill"), &files, &fs).unwrap();
        let on_disk = crate::checksum::checksum_dir(Path::new("/dest/skill"), &fs).unwrap();
        assert_eq!(
            expected, on_disk,
            "bundle and on-disk checksums must agree across .-vs-/ paths"
        );
    }
}
