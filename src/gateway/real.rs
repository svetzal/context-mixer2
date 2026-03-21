use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use mojentic::llm::gateways::{OllamaGateway, OpenAIGateway};
use mojentic::llm::{LlmBroker, LlmGateway, LlmMessage};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;

use super::clock::Clock;
use super::filesystem::{DirEntry, Filesystem};
use super::git::GitClient;
use super::llm::LlmClient;
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
pub struct MojenticLlmClient {
    config: LlmConfig,
}

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

impl LlmClient for MojenticLlmClient {
    fn analyze(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
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
