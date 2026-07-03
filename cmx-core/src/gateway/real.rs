use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::clock::Clock;
use super::filesystem::{DirEntry, Filesystem};
use super::git::GitClient;

#[cfg(feature = "llm")]
use mojentic::llm::gateways::{OllamaGateway, OpenAIGateway};
#[cfg(feature = "llm")]
use mojentic::llm::{LlmBroker, LlmGateway, LlmMessage};
#[cfg(feature = "llm")]
use std::future::Future;
#[cfg(feature = "llm")]
use std::pin::Pin;
#[cfg(feature = "llm")]
use std::sync::Arc;

#[cfg(feature = "llm")]
use super::llm::LlmClient;
#[cfg(feature = "llm")]
use crate::types::{LlmConfig, LlmGatewayType};

// ---------------------------------------------------------------------------
// RealFilesystem
// ---------------------------------------------------------------------------

/// Production implementation of [`Filesystem`] that delegates to `std::fs`.
pub struct RealFilesystem;

impl Filesystem for RealFilesystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))
    }

    fn write(&self, path: &Path, contents: &str) -> Result<()> {
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    fn write_bytes(&self, path: &Path, contents: &[u8]) -> Result<()> {
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory {}", path.display()))
    }

    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()> {
        std::fs::copy(src, dest)
            .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::rename(from, to)
            .with_context(|| format!("Failed to rename {} to {}", from.display(), to.display()))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove file {}", path.display()))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove directory {}", path.display()))
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let entries = std::fs::read_dir(path)
            .with_context(|| format!("Failed to read directory {}", path.display()))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry =
                entry.with_context(|| format!("Failed to read entry in {}", path.display()))?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path();
            let is_dir = entry_path.is_dir();
            result.push(DirEntry {
                path: entry_path,
                file_name,
                is_dir,
            });
        }

        Ok(result)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        path.canonicalize()
            .with_context(|| format!("Failed to canonicalize {}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// RealGitClient
// ---------------------------------------------------------------------------

/// Production implementation of [`GitClient`] that shells out to `git`.
pub struct RealGitClient;

impl GitClient for RealGitClient {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["clone", url, &dest.display().to_string()])
            .output()
            .context("Failed to run git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clone failed: {stderr}");
        }

        Ok(())
    }

    fn pull(&self, repo_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["-C", &repo_path.display().to_string(), "pull", "--quiet"])
            .output()
            .context("Failed to run git pull")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git pull failed: {stderr}");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SystemClock
// ---------------------------------------------------------------------------

/// Production implementation of [`Clock`] that returns `Utc::now()`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

// ---------------------------------------------------------------------------
// MojenticLlmClient
// ---------------------------------------------------------------------------

/// Production [`LlmClient`] that delegates to the mojentic LLM library.
#[cfg(feature = "llm")]
pub struct MojenticLlmClient {
    config: LlmConfig,
}

#[cfg(feature = "llm")]
impl MojenticLlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }

    fn make_broker(&self) -> LlmBroker {
        let gateway: Arc<dyn LlmGateway + Send + Sync> = match self.config.gateway {
            LlmGatewayType::OpenAI => Arc::new(OpenAIGateway::default()),
            LlmGatewayType::Ollama => Arc::new(OllamaGateway::new()),
        };
        LlmBroker::new(&self.config.model, gateway, None)
    }
}

#[cfg(feature = "llm")]
impl LlmClient for MojenticLlmClient {
    fn analyze(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        // not unit-tested: live network boundary
        let broker = self.make_broker();
        let messages = vec![
            LlmMessage::system(system_prompt),
            LlmMessage::user(user_prompt),
        ];
        Box::pin(async move {
            broker
                .generate(&messages, None, None, None)
                .await
                .context("LLM analysis failed")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // RealFilesystem
    // -----------------------------------------------------------------------

    #[test]
    fn real_filesystem_write_read_string_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("hello.txt");
        let fs = RealFilesystem;
        fs.write(&path, "hello world").unwrap();
        assert_eq!(fs.read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn real_filesystem_write_bytes_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bytes.bin");
        let fs = RealFilesystem;
        let data = vec![0u8, 1, 2, 255];
        fs.write_bytes(&path, &data).unwrap();
        assert_eq!(fs.read(&path).unwrap(), data);
    }

    #[test]
    fn real_filesystem_create_dir_all_nested() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a").join("b").join("c");
        let fs = RealFilesystem;
        fs.create_dir_all(&nested).unwrap();
        assert!(nested.is_dir());
    }

    #[test]
    fn real_filesystem_copy_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        let fs = RealFilesystem;
        fs.write(&src, "copy me").unwrap();
        fs.copy_file(&src, &dst).unwrap();
        assert_eq!(fs.read_to_string(&dst).unwrap(), "copy me");
        // source still exists
        assert!(fs.exists(&src));
    }

    #[test]
    fn real_filesystem_rename() {
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        let fs = RealFilesystem;
        fs.write(&from, "data").unwrap();
        fs.rename(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs.read_to_string(&to).unwrap(), "data");
    }

    #[test]
    fn real_filesystem_remove_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tmp.txt");
        let fs = RealFilesystem;
        fs.write(&path, "").unwrap();
        assert!(fs.exists(&path));
        fs.remove_file(&path).unwrap();
        assert!(!fs.exists(&path));
    }

    #[test]
    fn real_filesystem_remove_dir_all() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        let file = sub.join("file.txt");
        let fs = RealFilesystem;
        fs.create_dir_all(&sub).unwrap();
        fs.write(&file, "x").unwrap();
        fs.remove_dir_all(&sub).unwrap();
        assert!(!sub.exists());
    }

    #[test]
    fn real_filesystem_read_dir_entries() {
        let dir = TempDir::new().unwrap();
        let fs = RealFilesystem;
        fs.write(&dir.path().join("alpha.txt"), "a").unwrap();
        fs.write(&dir.path().join("beta.txt"), "b").unwrap();
        let entries = fs.read_dir(dir.path()).unwrap();
        let names: std::collections::BTreeSet<_> =
            entries.iter().map(|e| e.file_name.as_str()).collect();
        assert!(names.contains("alpha.txt"));
        assert!(names.contains("beta.txt"));
        // Verify DirEntry fields
        for entry in &entries {
            assert!(!entry.is_dir);
            assert!(entry.path.exists());
        }
    }

    #[test]
    fn real_filesystem_read_dir_contains_subdir_entry() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        let fs = RealFilesystem;
        fs.create_dir_all(&sub).unwrap();
        let entries = fs.read_dir(dir.path()).unwrap();
        let subdir_entry = entries.iter().find(|e| e.file_name == "subdir").unwrap();
        assert!(subdir_entry.is_dir);
    }

    #[test]
    fn real_filesystem_exists_is_dir_is_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        let fs = RealFilesystem;

        assert!(!fs.exists(&path));
        assert!(!fs.is_file(&path));
        assert!(!fs.is_dir(&path));

        fs.write(&path, "").unwrap();
        assert!(fs.exists(&path));
        assert!(fs.is_file(&path));
        assert!(!fs.is_dir(&path));

        assert!(fs.is_dir(dir.path()));
        assert!(!fs.is_file(dir.path()));
    }

    #[test]
    fn real_filesystem_canonicalize() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.txt");
        let fs = RealFilesystem;
        fs.write(&path, "").unwrap();
        let canonical = fs.canonicalize(&path).unwrap();
        assert!(canonical.is_absolute());
        assert!(canonical.exists());
    }

    #[test]
    fn real_filesystem_read_to_string_missing_returns_err() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("no_such_file.txt");
        let fs = RealFilesystem;
        let err = fs.read_to_string(&missing).unwrap_err();
        assert!(err.to_string().contains("Failed to read"));
    }

    // -----------------------------------------------------------------------
    // SystemClock
    // -----------------------------------------------------------------------

    #[test]
    fn system_clock_returns_current_wall_time() {
        let before = Utc::now();
        let clock = SystemClock;
        let result = clock.now();
        let after = Utc::now();
        assert!(result >= before, "clock result should be >= before");
        assert!(result <= after, "clock result should be <= after");
    }

    // -----------------------------------------------------------------------
    // RealGitClient
    // -----------------------------------------------------------------------

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    #[test]
    fn real_git_client_clone_and_pull() {
        if !git_available() {
            eprintln!("git not found on PATH — skipping RealGitClient tests");
            return;
        }

        let source_dir = TempDir::new().unwrap();
        // Init a bare-ish source repo with one commit
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(source_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(source_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(source_dir.path())
            .output()
            .unwrap();
        std::fs::write(source_dir.path().join("README.md"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(source_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(source_dir.path())
            .output()
            .unwrap();

        let clone_dir = TempDir::new().unwrap();
        let dest = clone_dir.path().join("repo");
        let client = RealGitClient;

        client.clone_repo(&source_dir.path().display().to_string(), &dest).unwrap();
        assert!(dest.join("README.md").exists());

        // pull should succeed on an up-to-date clone
        client.pull(&dest).unwrap();
    }

    #[test]
    fn real_git_client_clone_nonexistent_returns_err() {
        if !git_available() {
            eprintln!("git not found on PATH — skipping RealGitClient error test");
            return;
        }
        let dest = TempDir::new().unwrap();
        let client = RealGitClient;
        let err = client
            .clone_repo("/nonexistent/path/that/does/not/exist", dest.path())
            .unwrap_err();
        assert!(err.to_string().contains("git clone failed"));
    }

    // -----------------------------------------------------------------------
    // MojenticLlmClient (hermetic only — no network calls)
    // -----------------------------------------------------------------------

    #[cfg(feature = "llm")]
    #[test]
    fn mojentic_llm_client_new_stores_config() {
        use crate::types::{LlmConfig, LlmGatewayType};
        let config = LlmConfig {
            gateway: LlmGatewayType::OpenAI,
            model: "gpt-4o".to_string(),
        };
        let client = MojenticLlmClient::new(config.clone());
        assert_eq!(client.config.model, "gpt-4o");
        assert_eq!(client.config.gateway, LlmGatewayType::OpenAI);
    }

    #[cfg(feature = "llm")]
    #[test]
    fn mojentic_llm_client_make_broker_openai_does_not_panic() {
        use crate::types::{LlmConfig, LlmGatewayType};
        let client = MojenticLlmClient::new(LlmConfig {
            gateway: LlmGatewayType::OpenAI,
            model: "gpt-4o".to_string(),
        });
        let _broker = client.make_broker();
    }

    #[cfg(feature = "llm")]
    #[test]
    fn mojentic_llm_client_make_broker_ollama_does_not_panic() {
        use crate::types::{LlmConfig, LlmGatewayType};
        let client = MojenticLlmClient::new(LlmConfig {
            gateway: LlmGatewayType::Ollama,
            model: "llama3".to_string(),
        });
        let _broker = client.make_broker();
    }
}
