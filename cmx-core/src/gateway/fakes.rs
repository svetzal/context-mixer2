//! In-memory fakes for every gateway trait ([`Filesystem`], [`GitClient`], [`Clock`],
//! [`LlmClient`]), available under the `test-support` feature. Command logic written
//! against [`crate::context::AppContext`] runs unchanged against these fakes, so tests
//! never touch the real filesystem, network, or clock.

use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;

use super::clock::Clock;
use super::filesystem::{DirEntry, Filesystem};
use super::git::GitClient;
use super::llm::LlmClient;

use crate::error::{CmxError, GitOp, LlmError, Result};

// ---------------------------------------------------------------------------
// FakeFilesystem
// ---------------------------------------------------------------------------

/// In-memory filesystem for tests.
///
/// Files are stored as byte vectors keyed by their (absolute or relative)
/// path.  Directory entries are derived automatically from file paths.  All
/// methods that mutate state accept `&self` via interior mutability
/// (`RefCell`) so that test helpers and trait implementations share the same
/// borrow.
pub struct FakeFilesystem {
    files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    fail_on_write: RefCell<Option<PathBuf>>,
    fail_on_rename: RefCell<Option<PathBuf>>,
    fail_on_copy: RefCell<bool>,
}

impl FakeFilesystem {
    /// Create an empty fake filesystem with no files or directories.
    pub fn new() -> Self {
        Self {
            files: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            fail_on_write: RefCell::new(None),
            fail_on_rename: RefCell::new(None),
            fail_on_copy: RefCell::new(false),
        }
    }

    /// Cause the next `write()` to the given path to return an error.
    pub fn set_fail_on_write(&self, path: impl Into<PathBuf>) {
        *self.fail_on_write.borrow_mut() = Some(path.into());
    }

    /// Cause `rename()` targeting the given destination path to return an error.
    pub fn set_fail_on_rename(&self, path: impl Into<PathBuf>) {
        *self.fail_on_rename.borrow_mut() = Some(path.into());
    }

    /// Cause all `copy_file()` calls to return an error.
    pub fn set_fail_on_copy(&self, fail: bool) {
        *self.fail_on_copy.borrow_mut() = fail;
    }

    /// Insert a file with the given content, automatically registering all
    /// ancestor directories.
    pub fn add_file(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        let path = path.into();
        // Register all ancestor directories
        let mut current = path.parent();
        while let Some(parent) = current {
            if parent != Path::new("") {
                self.dirs.borrow_mut().insert(parent.to_path_buf());
            }
            current = parent.parent();
        }
        self.files.borrow_mut().insert(path, content.into());
    }

    /// Explicitly register a directory path.
    pub fn add_dir(&self, path: impl Into<PathBuf>) {
        self.dirs.borrow_mut().insert(path.into());
    }

    /// Return the stored content for a path, or `None` if absent.
    pub fn get_file_content(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.borrow().get(path).cloned()
    }

    /// Return true if the path has been added as a file.
    pub fn file_exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path)
    }

    /// Return a sorted snapshot of every file currently stored in the fake filesystem.
    pub fn snapshot_files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        self.files.borrow().clone()
    }
}

impl Default for FakeFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a `CmxError::Io` for a "not found" fake path.
fn not_found(context: String, path: &Path) -> CmxError {
    CmxError::Io {
        context,
        path: path.to_path_buf(),
        source: std::io::Error::new(ErrorKind::NotFound, "file not found"),
    }
}

/// Build a `CmxError::Io` for a "permission denied / injected failure" fake path.
fn fake_write_failure(path: &Path) -> CmxError {
    CmxError::Io {
        context: format!("Failed to write {}", path.display()),
        path: path.to_path_buf(),
        source: std::io::Error::new(ErrorKind::PermissionDenied, "injected write failure"),
    }
}

impl Filesystem for FakeFilesystem {
    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.borrow().contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path)
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        match self.files.borrow().get(path).cloned() {
            Some(bytes) => String::from_utf8(bytes).map_err(|e| CmxError::Io {
                context: format!("Failed to read {}", path.display()),
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            }),
            None => Err(not_found(format!("File not found: {}", path.display()), path)),
        }
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        match self.files.borrow().get(path).cloned() {
            Some(bytes) => Ok(bytes),
            None => Err(not_found(format!("File not found: {}", path.display()), path)),
        }
    }

    fn write(&self, path: &Path, contents: &str) -> Result<()> {
        if self.fail_on_write.borrow().as_deref() == Some(path) {
            return Err(fake_write_failure(path));
        }
        self.add_file(path.to_path_buf(), contents.as_bytes().to_vec());
        Ok(())
    }

    fn write_bytes(&self, path: &Path, contents: &[u8]) -> Result<()> {
        self.add_file(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let mut current = Some(path);
        while let Some(p) = current {
            if p != Path::new("") {
                self.dirs.borrow_mut().insert(p.to_path_buf());
            }
            current = p.parent();
        }
        Ok(())
    }

    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()> {
        if *self.fail_on_copy.borrow() {
            return Err(CmxError::Io {
                context: format!(
                    "FakeFilesystem: copy_file configured to fail ({} -> {})",
                    src.display(),
                    dest.display()
                ),
                path: dest.to_path_buf(),
                source: std::io::Error::new(ErrorKind::PermissionDenied, "injected copy failure"),
            });
        }
        let bytes =
            self.files.borrow().get(src).cloned().ok_or_else(|| {
                not_found(format!("Source file not found: {}", src.display()), src)
            })?;
        self.add_file(dest.to_path_buf(), bytes);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        if self.fail_on_rename.borrow().as_deref() == Some(to) {
            return Err(CmxError::Io {
                context: format!("FakeFilesystem: rename configured to fail for {}", to.display()),
                path: to.to_path_buf(),
                source: std::io::Error::new(ErrorKind::PermissionDenied, "injected rename failure"),
            });
        }
        let bytes = self.files.borrow().get(from).cloned().ok_or_else(|| {
            not_found(format!("Source file not found for rename: {}", from.display()), from)
        })?;
        self.add_file(to.to_path_buf(), bytes);
        self.files.borrow_mut().remove(from);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        if self.files.borrow_mut().remove(path).is_none() {
            return Err(not_found(format!("File not found: {}", path.display()), path));
        }
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        // Remove all files whose path starts with `path`
        let prefix = path.to_path_buf();
        self.files.borrow_mut().retain(|k, _| !k.starts_with(&prefix));
        // Remove all dirs whose path starts with `path`
        self.dirs.borrow_mut().retain(|k| !k.starts_with(&prefix));
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        if !self.dirs.borrow().contains(path) && !self.files.borrow().contains_key(path) {
            return Err(CmxError::Io {
                context: format!("Failed to read directory {}", path.display()),
                path: path.to_path_buf(),
                source: std::io::Error::new(ErrorKind::NotFound, "directory not found"),
            });
        }

        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let mut entries = Vec::new();

        // Collect immediate children from files
        for file_path in self.files.borrow().keys() {
            if let Some(parent) = file_path.parent()
                && parent == path
            {
                let file_name = file_path
                    .file_name()
                    .expect("file path with a matched parent must have a final component")
                    .to_string_lossy()
                    .to_string();
                if seen.insert(file_path.clone()) {
                    entries.push(DirEntry {
                        path: file_path.clone(),
                        file_name,
                        is_dir: false,
                    });
                }
            }
        }

        // Collect immediate children from dirs
        for dir_path in self.dirs.borrow().clone() {
            if let Some(parent) = dir_path.parent()
                && parent == path
                && seen.insert(dir_path.clone())
            {
                let file_name = dir_path
                    .file_name()
                    .expect("dir path with a matched parent must have a final component")
                    .to_string_lossy()
                    .to_string();
                entries.push(DirEntry {
                    path: dir_path,
                    file_name,
                    is_dir: true,
                });
            }
        }

        Ok(entries)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        // In tests we treat the path as already canonical
        Ok(path.to_path_buf())
    }
}

// ---------------------------------------------------------------------------
// FakeGitClient
// ---------------------------------------------------------------------------

/// Records git operations without executing them.
pub struct FakeGitClient {
    /// Every `(url, dest)` pair passed to `clone_repo`, in call order.
    pub cloned: RefCell<Vec<(String, PathBuf)>>,
    /// Every repo path passed to `pull`, in call order.
    pub pulled: RefCell<Vec<PathBuf>>,
    /// When `true`, every operation returns a `CmxError::Git` error instead of succeeding.
    pub should_fail: bool,
}

impl FakeGitClient {
    /// Create a fake git client with no recorded calls, configured to succeed.
    pub fn new() -> Self {
        Self {
            cloned: RefCell::new(Vec::new()),
            pulled: RefCell::new(Vec::new()),
            should_fail: false,
        }
    }
}

impl Default for FakeGitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClient for FakeGitClient {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<()> {
        if self.should_fail {
            return Err(CmxError::Git {
                operation: GitOp::Clone,
                stderr: "FakeGitClient: clone_repo configured to fail".to_string(),
            });
        }
        self.cloned.borrow_mut().push((url.to_string(), dest.to_path_buf()));
        Ok(())
    }

    fn pull(&self, repo_path: &Path) -> Result<()> {
        if self.should_fail {
            return Err(CmxError::Git {
                operation: GitOp::Pull,
                stderr: "FakeGitClient: pull configured to fail".to_string(),
            });
        }
        self.pulled.borrow_mut().push(repo_path.to_path_buf());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FakeClock
// ---------------------------------------------------------------------------

/// Always returns the same instant.
pub struct FakeClock {
    /// The fixed instant `now()` always returns.
    pub now: DateTime<Utc>,
}

impl FakeClock {
    /// Create a fake clock fixed at `now`.
    pub fn at(now: DateTime<Utc>) -> Self {
        Self { now }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }
}

// ---------------------------------------------------------------------------
// FakeLlmClient
// ---------------------------------------------------------------------------

/// Returns a canned response string, or fails if `should_fail` is set.
/// Also records every `(system_prompt, user_prompt)` pair passed to `analyze`.
pub struct FakeLlmClient {
    /// The canned string `analyze` returns when `should_fail` is `false`.
    pub response: String,
    /// When `true`, `analyze` returns an error instead of `response`.
    pub should_fail: bool,
    /// Every `(system_prompt, user_prompt)` pair passed to `analyze`, in call order.
    pub calls: Mutex<Vec<(String, String)>>,
}

impl FakeLlmClient {
    /// Create a fake LLM client that succeeds with the given canned `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            should_fail: false,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Return the most recent `(system_prompt, user_prompt)` pair, if any.
    pub fn last_call(&self) -> Option<(String, String)> {
        self.calls.lock().unwrap().last().cloned()
    }

    /// Return all recorded `(system_prompt, user_prompt)` pairs in call order.
    pub fn all_calls(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}

impl LlmClient for FakeLlmClient {
    fn analyze(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        self.calls
            .lock()
            .unwrap()
            .push((system_prompt.to_string(), user_prompt.to_string()));
        let should_fail = self.should_fail;
        let response = self.response.clone();
        Box::pin(async move {
            if should_fail {
                return Err(CmxError::Llm(LlmError::Other(
                    "FakeLlmClient: analyze configured to fail".to_string(),
                )));
            }
            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_filesystem_add_file_and_exists() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/home/user/test.txt");
        fs.add_file(path.clone(), b"hello".to_vec());
        assert!(fs.exists(&path));
        assert!(fs.is_file(&path));
        assert!(!fs.is_dir(&path));
    }

    #[test]
    fn fake_filesystem_add_file_registers_parent_dirs() {
        let fs = FakeFilesystem::new();
        fs.add_file("/home/user/subdir/file.txt", "content");
        assert!(fs.is_dir(&PathBuf::from("/home/user/subdir")));
        assert!(fs.is_dir(&PathBuf::from("/home/user")));
        assert!(fs.is_dir(&PathBuf::from("/home")));
    }

    #[test]
    fn fake_filesystem_read_to_string_returns_content() {
        let fs = FakeFilesystem::new();
        fs.add_file("/tmp/a.txt", "hello world");
        let content = fs.read_to_string(Path::new("/tmp/a.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn fake_filesystem_read_missing_file_errors() {
        let fs = FakeFilesystem::new();
        let err = fs.read_to_string(Path::new("/nonexistent.txt")).unwrap_err();
        assert!(matches!(err, CmxError::Io { .. }));
    }

    #[test]
    fn fake_filesystem_write_then_read() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/tmp/out.txt");
        fs.write(&path, "written content").unwrap();
        assert_eq!(fs.read_to_string(&path).unwrap(), "written content");
    }

    #[test]
    fn fake_filesystem_remove_file() {
        let fs = FakeFilesystem::new();
        let path = PathBuf::from("/tmp/to_remove.txt");
        fs.add_file(path.clone(), "data");
        fs.remove_file(&path).unwrap();
        assert!(!fs.exists(&path));
    }

    #[test]
    fn fake_filesystem_remove_dir_all_removes_children() {
        let fs = FakeFilesystem::new();
        fs.add_file("/skills/my-skill/SKILL.md", "---\n---\n");
        fs.add_file("/skills/my-skill/tool.py", "code");
        fs.remove_dir_all(Path::new("/skills/my-skill")).unwrap();
        assert!(!fs.exists(Path::new("/skills/my-skill/SKILL.md")));
        assert!(!fs.exists(Path::new("/skills/my-skill/tool.py")));
    }

    #[test]
    fn fake_filesystem_read_dir_lists_children() {
        let fs = FakeFilesystem::new();
        fs.add_file("/agents/alpha.md", "# agent");
        fs.add_file("/agents/beta.md", "# agent");
        let entries = fs.read_dir(Path::new("/agents")).unwrap();
        let names: BTreeSet<_> = entries.iter().map(|e| e.file_name.as_str()).collect();
        assert!(names.contains("alpha.md"));
        assert!(names.contains("beta.md"));
    }

    #[test]
    fn fake_git_client_records_clone() {
        let git = FakeGitClient::new();
        git.clone_repo("https://example.com/repo.git", Path::new("/tmp/repo")).unwrap();
        let cloned = git.cloned.borrow();
        assert_eq!(cloned.len(), 1);
        assert_eq!(cloned[0].0, "https://example.com/repo.git");
    }

    #[test]
    fn fake_git_client_records_pull() {
        let git = FakeGitClient::new();
        git.pull(Path::new("/tmp/repo")).unwrap();
        let pulled = git.pulled.borrow();
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0], PathBuf::from("/tmp/repo"));
    }

    #[test]
    fn fake_git_client_clone_fail_returns_typed_git_error() {
        let git = FakeGitClient {
            should_fail: true,
            ..FakeGitClient::new()
        };
        let err = git.clone_repo("url", Path::new("/tmp")).unwrap_err();
        assert!(
            matches!(
                err,
                CmxError::Git {
                    operation: GitOp::Clone,
                    ..
                }
            ),
            "expected Git(Clone), got {err:?}"
        );
    }

    #[test]
    fn fake_git_client_pull_fail_returns_typed_git_error() {
        let git = FakeGitClient {
            should_fail: true,
            ..FakeGitClient::new()
        };
        let err = git.pull(Path::new("/tmp/repo")).unwrap_err();
        assert!(
            matches!(
                err,
                CmxError::Git {
                    operation: GitOp::Pull,
                    ..
                }
            ),
            "expected Git(Pull), got {err:?}"
        );
    }

    #[test]
    fn fake_clock_returns_fixed_time() {
        use chrono::TimeZone;
        let fixed = Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let clock = FakeClock::at(fixed);
        assert_eq!(clock.now(), fixed);
    }

    #[tokio::test]
    async fn fake_llm_client_returns_canned_response() {
        let client = FakeLlmClient::new("This is the analysis.");
        let result = client.analyze("system", "user").await.unwrap();
        assert_eq!(result, "This is the analysis.");
    }

    #[tokio::test]
    async fn fake_llm_client_captures_prompts() {
        let client = FakeLlmClient::new("result");
        assert!(client.last_call().is_none(), "no calls yet");
        client.analyze("sys", "usr").await.unwrap();
        assert_eq!(
            client.last_call(),
            Some(("sys".to_string(), "usr".to_string())),
            "captures the (system, user) pair"
        );
        client.analyze("sys2", "usr2").await.unwrap();
        assert_eq!(client.all_calls().len(), 2, "accumulates calls");
        assert_eq!(
            client.last_call(),
            Some(("sys2".to_string(), "usr2".to_string())),
            "last_call returns the most recent"
        );
    }

    #[test]
    fn fake_filesystem_write_fails_on_configured_path() {
        let fs = FakeFilesystem::new();
        let fail_path = PathBuf::from("/config/restricted.json");
        let other_path = PathBuf::from("/config/allowed.json");

        fs.set_fail_on_write(fail_path.clone());

        // Write to the configured fail path returns a typed Io error
        let err = fs.write(&fail_path, "data").unwrap_err();
        assert!(matches!(err, CmxError::Io { .. }));

        // Write to a different path still succeeds
        assert!(fs.write(&other_path, "data").is_ok());
        assert_eq!(fs.read_to_string(&other_path).unwrap(), "data");
    }

    #[test]
    fn fake_filesystem_copy_fails_when_configured() {
        let fs = FakeFilesystem::new();
        let src = PathBuf::from("/src/file.txt");
        let dest = PathBuf::from("/dest/file.txt");
        fs.add_file(src.clone(), "content");

        fs.set_fail_on_copy(true);

        let err = fs.copy_file(&src, &dest).unwrap_err();
        assert!(matches!(err, CmxError::Io { .. }));
        // Verify nothing was copied
        assert!(!fs.file_exists(&dest));
    }

    #[tokio::test]
    async fn fake_llm_client_fails_when_configured() {
        let client = FakeLlmClient {
            response: "unreachable".to_string(),
            should_fail: true,
            calls: Mutex::new(Vec::new()),
        };
        let result = client.analyze("system", "user").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Typed: CmxError::Llm(LlmError::Other(_))
        assert!(
            matches!(err, CmxError::Llm(LlmError::Other(_))),
            "expected Llm(Other), got {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("configured to fail"), "unexpected: {msg}");
    }

    #[test]
    fn fake_filesystem_rename_moves_file_and_removes_source() {
        let fs = FakeFilesystem::new();
        let from = PathBuf::from("/tmp/source.txt");
        let to = PathBuf::from("/tmp/dest.txt");
        fs.add_file(from.clone(), "content");

        fs.rename(&from, &to).unwrap();

        assert!(!fs.file_exists(&from), "source should be removed after rename");
        assert!(fs.file_exists(&to), "destination should exist after rename");
        assert_eq!(fs.read_to_string(&to).unwrap(), "content");
    }

    #[test]
    fn fake_filesystem_rename_replaces_existing_destination() {
        let fs = FakeFilesystem::new();
        let from = PathBuf::from("/tmp/source.txt");
        let to = PathBuf::from("/tmp/dest.txt");
        fs.add_file(from.clone(), "new content");
        fs.add_file(to.clone(), "old content");

        fs.rename(&from, &to).unwrap();

        assert!(!fs.file_exists(&from));
        assert_eq!(fs.read_to_string(&to).unwrap(), "new content");
    }

    #[test]
    fn fake_filesystem_rename_fails_on_configured_destination() {
        let fs = FakeFilesystem::new();
        let from = PathBuf::from("/tmp/source.txt");
        let to = PathBuf::from("/tmp/restricted.txt");
        fs.add_file(from.clone(), "content");
        fs.set_fail_on_rename(to.clone());

        let err = fs.rename(&from, &to).unwrap_err();
        assert!(matches!(err, CmxError::Io { .. }));
        // Source file should be untouched after failed rename
        assert!(fs.file_exists(&from), "source should remain after failed rename");
    }

    #[test]
    fn fake_filesystem_rename_errors_when_source_absent() {
        let fs = FakeFilesystem::new();
        let from = PathBuf::from("/tmp/nonexistent.txt");
        let to = PathBuf::from("/tmp/dest.txt");

        let result = fs.rename(&from, &to);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("nonexistent.txt"), "unexpected: {msg}");
    }
}
