//! Production gateway implementations that perform real I/O: [`RealFilesystem`]
//! (delegates to `std::fs`), [`RealGitClient`] (shells out to `git`), [`SystemClock`]
//! (wall-clock time), and, under feature `llm`, `MojenticLlmClient` (the mojentic
//! crate's LLM broker). Wired together by [`crate::production::ProductionContext`].

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::clock::Clock;
use super::filesystem::{DirEntry, Filesystem};
use super::git::GitClient;

use crate::error::{CmxError, GitOp, Result};

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
use crate::error::LlmError;
#[cfg(feature = "llm")]
use crate::types::{LlmConfig, LlmGatewayType};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Construct `CmxError::Io` for a single-path operation.
#[inline]
fn io_err(op: &str, path: &Path, source: std::io::Error) -> CmxError {
    CmxError::Io {
        context: format!("Failed to {op} {}", path.display()),
        path: path.to_path_buf(),
        source,
    }
}

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
        std::fs::read_to_string(path).map_err(|e| io_err("read", path, e))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).map_err(|e| io_err("read", path, e))
    }

    fn write(&self, path: &Path, contents: &str) -> Result<()> {
        std::fs::write(path, contents).map_err(|e| io_err("write", path, e))
    }

    fn write_bytes(&self, path: &Path, contents: &[u8]) -> Result<()> {
        std::fs::write(path, contents).map_err(|e| io_err("write", path, e))
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(|e| io_err("create directory", path, e))
    }

    fn copy_file(&self, src: &Path, dest: &Path) -> Result<()> {
        std::fs::copy(src, dest)
            .map_err(|e| CmxError::Io {
                context: format!("Failed to copy {} to {}", src.display(), dest.display()),
                path: dest.to_path_buf(),
                source: e,
            })
            .map(|_| ())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::rename(from, to).map_err(|e| CmxError::Io {
            context: format!("Failed to rename {} to {}", from.display(), to.display()),
            path: to.to_path_buf(),
            source: e,
        })
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).map_err(|e| io_err("remove file", path, e))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path).map_err(|e| io_err("remove directory", path, e))
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let entries = std::fs::read_dir(path).map_err(|e| io_err("read directory", path, e))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io_err("read entry in", path, e))?;
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
        path.canonicalize().map_err(|e| io_err("canonicalize", path, e))
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
            .map_err(|e| io_err("run git clone", dest, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(CmxError::Git {
                operation: GitOp::Clone,
                stderr,
            });
        }

        Ok(())
    }

    fn pull(&self, repo_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["-C", &repo_path.display().to_string(), "pull", "--quiet"])
            .output()
            .map_err(|e| io_err("run git pull", repo_path, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(CmxError::Git {
                operation: GitOp::Pull,
                stderr,
            });
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
    /// Build a client for the gateway and model described by `config`.
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
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<String>> + Send + '_>> {
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
                .map_err(|e| CmxError::Llm(classify_mojentic_error(&e.into())))
        })
    }
}

/// Translate a mojentic `anyhow::Error` into the typed [`LlmError`] taxonomy.
///
/// This is the **single location** in the codebase where string inspection of
/// an opaque error chain is permitted.  It confines the pattern-matching that
/// `error_summary` previously applied everywhere to a tested, documented
/// boundary adapter.
///
/// # Classification rules
///
/// 1. If the formatted chain contains `" API error:"` preceded by a provider
///    name, classify as [`LlmError::Provider`].
/// 2. If the chain mentions an Ollama host and a connection failure, classify
///    as [`LlmError::Unreachable`].
/// 3. Everything else falls through to [`LlmError::Other`].
#[cfg(feature = "llm")]
pub fn classify_mojentic_error(e: &anyhow::Error) -> LlmError {
    let text = format!("{e:#}");

    // Strip any wrapper context like "LLM analysis failed: LLM gateway error: "
    let stripped = strip_wrapper_prefixes(&text);

    // Provider error: look for "OpenAI API error: 401 Unauthorized" style
    if let Some(provider_err) = extract_provider_error(stripped) {
        return provider_err;
    }

    // Ollama unreachable
    if looks_like_ollama_unreachable(stripped) {
        let endpoint =
            extract_ollama_endpoint(stripped).unwrap_or_else(|| "localhost:11434".to_string());
        return LlmError::Unreachable { endpoint };
    }

    LlmError::Other(truncate(stripped, 200))
}

#[cfg(feature = "llm")]
const WRAPPER_PREFIXES: &[&str] = &["LLM analysis failed:", "LLM gateway error:"];

#[cfg(feature = "llm")]
fn strip_wrapper_prefixes(mut text: &str) -> &str {
    loop {
        let mut stripped = false;
        for prefix in WRAPPER_PREFIXES {
            if let Some(rest) = text.strip_prefix(prefix) {
                text = rest.trim_start();
                stripped = true;
                break;
            }
        }
        if !stripped {
            return text;
        }
    }
}

#[cfg(feature = "llm")]
fn extract_provider_error(text: &str) -> Option<LlmError> {
    // Matches: "OpenAI API error: 401 Unauthorized - {json body}"
    let api_error_pos = text.find(" API error:")?;
    // Extract provider: everything before " API error:"
    let provider = text[..api_error_pos].split_whitespace().last()?.to_string();
    let after_marker = text[api_error_pos + " API error:".len()..].trim();
    // Strip trailing JSON body starting at " - {"
    let message_end = after_marker.find(" - {").unwrap_or(after_marker.len());
    let message = after_marker[..message_end].trim_end_matches([' ', ':']).to_string();
    // Parse optional status code as the first token in message
    let status = message.split_whitespace().next().and_then(|s| s.parse::<u16>().ok());
    Some(LlmError::Provider {
        provider,
        status,
        message,
    })
}

#[cfg(feature = "llm")]
fn looks_like_ollama_unreachable(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("localhost:11434") || lower.contains("ollama"))
        && (lower.contains("connection refused")
            || lower.contains("failed to connect")
            || lower.contains("error sending request")
            || lower.contains("tcp connect error"))
}

#[cfg(feature = "llm")]
fn extract_ollama_endpoint(text: &str) -> Option<String> {
    // Try to find "localhost:NNNNN" in the message
    let lower = text.to_ascii_lowercase();
    let start = lower.find("localhost:")?;
    let port_start = start + "localhost:".len();
    let port_end = lower[port_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(lower.len(), |i| port_start + i);
    if port_start < port_end {
        Some(format!("localhost:{}", &text[port_start..port_end]))
    } else {
        Some("localhost:11434".to_string())
    }
}

#[cfg(feature = "llm")]
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max).collect();
        format!("{head}…")
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
        // Typed: CmxError::Io
        assert!(matches!(err, CmxError::Io { .. }));
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
        // Typed: CmxError::Git
        assert!(matches!(
            err,
            CmxError::Git {
                operation: GitOp::Clone,
                ..
            }
        ));
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

    // -----------------------------------------------------------------------
    // classify_mojentic_error
    // -----------------------------------------------------------------------

    #[cfg(feature = "llm")]
    #[test]
    fn classify_openai_provider_error() {
        let raw = anyhow::anyhow!(
            "LLM analysis failed: LLM gateway error: OpenAI API error: 401 Unauthorized - \
             {{\"error\":{{\"message\":\"No API key\"}}}}"
        );
        let classified = classify_mojentic_error(&raw);
        assert!(
            matches!(classified, LlmError::Provider { .. }),
            "expected Provider, got {classified:?}"
        );
        if let LlmError::Provider {
            provider,
            status,
            message,
        } = classified
        {
            assert_eq!(provider, "OpenAI");
            assert_eq!(status, Some(401));
            assert!(message.contains("Unauthorized"), "message: {message}");
        }
    }

    #[cfg(feature = "llm")]
    #[test]
    fn classify_ollama_unreachable() {
        let raw = anyhow::anyhow!(
            "LLM gateway error: error sending request for url (http://localhost:11434/api/chat): \
             connection refused"
        );
        let classified = classify_mojentic_error(&raw);
        assert!(
            matches!(classified, LlmError::Unreachable { .. }),
            "expected Unreachable, got {classified:?}"
        );
    }

    #[cfg(feature = "llm")]
    #[test]
    fn classify_unknown_error_falls_through_to_other() {
        let raw = anyhow::anyhow!("some totally unexpected error");
        let classified = classify_mojentic_error(&raw);
        assert!(matches!(classified, LlmError::Other(_)), "expected Other, got {classified:?}");
    }
}
