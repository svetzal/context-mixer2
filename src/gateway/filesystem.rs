use anyhow::Result;
use std::path::{Path, PathBuf};

/// A simplified directory entry for the `Filesystem` trait boundary.
///
/// Uses a custom type (not `std::fs::DirEntry`) so that test fakes can
/// construct instances without touching the real filesystem.
pub struct DirEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub is_dir: bool,
}

/// Abstraction over filesystem I/O operations used by cmx.
///
/// Every function that reads or writes files accepts `&dyn Filesystem` rather
/// than calling `std::fs` directly.  Production code uses [`RealFilesystem`];
/// tests use the in-memory fake defined in [`super::fakes`].
pub trait Filesystem {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Result<String>;
    fn read(&self, path: &Path) -> Result<Vec<u8>>;
    fn write(&self, path: &Path, contents: &str) -> Result<()>;
    fn write_bytes(&self, path: &Path, contents: &[u8]) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()>;
    fn remove_file(&self, path: &Path) -> Result<()>;
    fn remove_dir_all(&self, path: &Path) -> Result<()>;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf>;
}
