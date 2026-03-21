use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use super::clock::Clock;
use super::filesystem::{DirEntry, Filesystem};
use super::git::GitClient;
use super::llm::LlmClient;

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
}

impl FakeFilesystem {
    pub fn new() -> Self {
        Self {
            files: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
        }
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
}

impl Default for FakeFilesystem {
    fn default() -> Self {
        Self::new()
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
            Some(bytes) => Ok(String::from_utf8(bytes)?),
            None => bail!("File not found: {}", path.display()),
        }
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        match self.files.borrow().get(path).cloned() {
            Some(bytes) => Ok(bytes),
            None => bail!("File not found: {}", path.display()),
        }
    }

    fn write(&self, path: &Path, contents: &str) -> Result<()> {
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
        let bytes = self
            .files
            .borrow()
            .get(src)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Source file not found: {}", src.display()))?;
        self.add_file(dest.to_path_buf(), bytes);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        if self.files.borrow_mut().remove(path).is_none() {
            bail!("File not found: {}", path.display());
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
            bail!("Directory not found: {}", path.display());
        }

        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let mut entries = Vec::new();

        // Collect immediate children from files
        for file_path in self.files.borrow().keys() {
            if let Some(parent) = file_path.parent()
                && parent == path
            {
                let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
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
                let file_name = dir_path.file_name().unwrap().to_string_lossy().to_string();
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
    pub cloned: RefCell<Vec<(String, PathBuf)>>,
    pub pulled: RefCell<Vec<PathBuf>>,
    pub should_fail: bool,
}

impl FakeGitClient {
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
            bail!("FakeGitClient: clone_repo configured to fail");
        }
        self.cloned.borrow_mut().push((url.to_string(), dest.to_path_buf()));
        Ok(())
    }

    fn pull(&self, repo_path: &Path) -> Result<()> {
        if self.should_fail {
            bail!("FakeGitClient: pull configured to fail");
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
    pub now: DateTime<Utc>,
}

impl FakeClock {
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

/// Returns a canned response string.
pub struct FakeLlmClient {
    pub response: String,
}

impl FakeLlmClient {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl LlmClient for FakeLlmClient {
    fn analyze(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let response = self.response.clone();
        Box::pin(async move { Ok(response) })
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
        assert!(fs.read_to_string(Path::new("/nonexistent.txt")).is_err());
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
}
