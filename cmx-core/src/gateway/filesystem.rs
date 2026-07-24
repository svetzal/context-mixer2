//! The [`Filesystem`] gateway trait — every file read/write cmx-core performs goes
//! through here, never through `std::fs` directly.

use crate::error::Result;
use std::path::{Path, PathBuf};

/// A simplified directory entry for the `Filesystem` trait boundary.
///
/// Uses a custom type (not `std::fs::DirEntry`) so that test fakes can
/// construct instances without touching the real filesystem.
pub struct DirEntry {
    /// The entry's full path.
    pub path: PathBuf,
    /// The entry's bare file or directory name (the last path component).
    pub file_name: String,
    /// Whether the entry is a directory (`true`) or a file (`false`).
    pub is_dir: bool,
}

/// Abstraction over filesystem I/O operations used by cmx.
///
/// Every function that reads or writes files accepts `&dyn Filesystem` rather
/// than calling `std::fs` directly.  Production code uses `RealFilesystem`;
/// tests use the in-memory fake defined in [`super::fakes`].
pub trait Filesystem {
    /// Whether `path` exists (as a file or a directory).
    fn exists(&self, path: &Path) -> bool;
    /// Whether `path` exists and is a directory.
    fn is_dir(&self, path: &Path) -> bool;
    /// Whether `path` exists and is a regular file.
    fn is_file(&self, path: &Path) -> bool;
    /// Read the file at `path` as a UTF-8 string.
    fn read_to_string(&self, path: &Path) -> Result<String>;
    /// Read the file at `path` as raw bytes.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;
    /// Write `contents` to `path`, creating or truncating the file.
    fn write(&self, path: &Path, contents: &str) -> Result<()>;
    /// Write raw `contents` to `path`, creating or truncating the file.
    fn write_bytes(&self, path: &Path, contents: &[u8]) -> Result<()>;
    /// Create `path` and any missing parent directories.
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    /// Copy the file at `src` to `dest`, overwriting `dest` if it exists.
    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()>;
    /// Atomically rename `from` to `to`, replacing `to` if it already exists.
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;
    /// Delete the file at `path`.
    fn remove_file(&self, path: &Path) -> Result<()>;
    /// Recursively delete the directory at `path` and everything under it.
    fn remove_dir_all(&self, path: &Path) -> Result<()>;
    /// List the immediate entries of the directory at `path`.
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;
    /// Resolve `path` to its canonical, absolute, symlink-free form.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf>;
}
