//! Typed, matchable domain errors for cmx-core.
//!
//! All public fallible APIs return [`Result<T>`] (a type alias for
//! `core::result::Result<T, CmxError>`).  Embedders can match on specific
//! variants without any string inspection:
//!
//! ```
//! # use cmx_core::error::{CmxError, Result};
//! # use cmx_core::types::SourcesFile;
//! let err: CmxError = SourcesFile::default().get_source("nope").err().unwrap();
//! assert!(matches!(err, CmxError::SourceNotFound { .. }));
//! assert_eq!(err.code(), "source-not-found");
//! ```

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// LlmError
// ---------------------------------------------------------------------------

/// Classification of LLM-gateway failures.
///
/// Produced by [`classify_mojentic_error`](crate::gateway::real::classify_mojentic_error)
/// at the mojentic boundary in production; constructed directly in fakes.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LlmError {
    /// The provider returned a recognised HTTP error response.
    ///
    /// `status` is available for programmatic matching; it is not included in
    /// the `Display` output because `Option<u16>` does not implement `Display`.
    /// The HTTP status code is already present in `message` when produced by
    /// [`classify_mojentic_error`](crate::gateway::real::classify_mojentic_error).
    #[error("{provider} API error: {message}")]
    Provider {
        provider: String,
        /// HTTP status code, when present in the error body.
        status: Option<u16>,
        message: String,
    },

    /// The LLM endpoint could not be reached.
    #[error("Ollama unreachable at {endpoint}")]
    Unreachable { endpoint: String },

    /// Any other LLM error not matching the patterns above.
    #[error("{0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// GitOp
// ---------------------------------------------------------------------------

/// The git operation that produced a [`CmxError::Git`] error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOp {
    Clone,
    Pull,
}

impl std::fmt::Display for GitOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitOp::Clone => write!(f, "clone"),
            GitOp::Pull => write!(f, "pull"),
        }
    }
}

// ---------------------------------------------------------------------------
// CmxError
// ---------------------------------------------------------------------------

/// Typed domain errors returned by all public cmx-core APIs.
///
/// Every variant carries a stable [`code()`](CmxError::code) string that
/// the TypeScript port mirrors and the conformance suite pins.
#[derive(Debug, thiserror::Error)]
pub enum CmxError {
    /// I/O error on a specific filesystem path.
    ///
    /// `context` carries the full "Failed to \<verb\> \<path\>" prefix
    /// byte-for-byte identical to the previous `with_context` message, so
    /// CLI output and tests that substring-match do not change.
    #[error("{context}: {source}")]
    Io {
        context: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// JSON parse error while reading a specific file.
    #[error("{context}: {source}")]
    Json {
        context: String,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// The OS home directory could not be determined.
    #[error("Could not determine home directory")]
    HomeDirUnavailable,

    /// The active platform does not support the requested artifact kind.
    #[error(
        "The {platform} platform does not support {kind}s. \
         {platform} has no native {kind} concept."
    )]
    UnsupportedArtifact {
        platform: crate::platform::Platform,
        kind: crate::types::ArtifactKind,
    },

    /// A named source was not found in the sources file.
    #[error("Source '{name}' not found.")]
    SourceNotFound { name: String },

    /// A source entry is missing its required path configuration.
    ///
    /// The `msg` field carries the byte-identical legacy message so that
    /// existing test assertions on the string representation still pass.
    #[error("{msg}")]
    SourcePathMissing {
        msg: &'static str,
        kind: crate::types::SourceType,
    },

    /// A bundled skill does not contain the required `SKILL.md`.
    #[error("BundledSkill for '{skill}' is missing SKILL.md")]
    MissingSkillMd { skill: String },

    /// An install plan is blocked (e.g. trying to install older than what's locked).
    #[error("Install plan for '{tool}' is blocked. Run with force=true to override.")]
    VersionGuard { tool: String },

    /// The `BundledSkill` changed between `plan()` and `apply()` (parity guard).
    #[error(
        "Parity check failed for '{tool}': \
         the BundledSkill has changed since plan() was called."
    )]
    Drift { tool: String },

    /// A `git` subprocess exited with a non-zero status.
    #[error("git {operation} failed: {stderr}")]
    Git { operation: GitOp, stderr: String },

    /// An LLM gateway error.
    #[error(transparent)]
    Llm(#[from] LlmError),
}

impl CmxError {
    /// Stable kebab-case discriminant for this error variant.
    ///
    /// This is the token the TypeScript port mirrors and the conformance suite
    /// pins. It never changes for an existing variant.
    pub fn code(&self) -> &'static str {
        match self {
            CmxError::Io { .. } => "io",
            CmxError::Json { .. } => "json",
            CmxError::HomeDirUnavailable => "home-dir-unavailable",
            CmxError::UnsupportedArtifact { .. } => "unsupported-artifact",
            CmxError::SourceNotFound { .. } => "source-not-found",
            CmxError::SourcePathMissing { .. } => "source-path-missing",
            CmxError::MissingSkillMd { .. } => "missing-skill-md",
            CmxError::VersionGuard { .. } => "version-guard",
            CmxError::Drift { .. } => "drift",
            CmxError::Git { .. } => "git",
            CmxError::Llm(_) => "llm",
        }
    }
}

// ---------------------------------------------------------------------------
// Result alias
// ---------------------------------------------------------------------------

/// cmx-core's canonical result type — `core::result::Result<T, CmxError>`.
///
/// Import as `use cmx_core::error::Result;` in embedder code.
pub type Result<T> = core::result::Result<T, CmxError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_error_provider_formats_correctly() {
        let e = LlmError::Provider {
            provider: "OpenAI".to_string(),
            status: Some(401),
            message: "Unauthorized".to_string(),
        };
        assert_eq!(e.to_string(), "OpenAI API error: Unauthorized");
    }

    #[test]
    fn llm_error_unreachable_formats_correctly() {
        let e = LlmError::Unreachable {
            endpoint: "localhost:11434".to_string(),
        };
        assert_eq!(e.to_string(), "Ollama unreachable at localhost:11434");
    }

    #[test]
    fn llm_error_other_formats_correctly() {
        let e = LlmError::Other("something went wrong".to_string());
        assert_eq!(e.to_string(), "something went wrong");
    }

    #[test]
    fn cmx_error_code_stable() {
        let io_err = CmxError::Io {
            context: "Failed to read /x".to_string(),
            path: PathBuf::from("/x"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert_eq!(io_err.code(), "io");

        let json_err = CmxError::Json {
            context: "Failed to parse /x".to_string(),
            path: PathBuf::from("/x"),
            source: serde_json::from_str::<()>("invalid").unwrap_err(),
        };
        assert_eq!(json_err.code(), "json");

        assert_eq!(CmxError::HomeDirUnavailable.code(), "home-dir-unavailable");

        assert_eq!(
            CmxError::SourceNotFound {
                name: "x".to_string()
            }
            .code(),
            "source-not-found"
        );

        assert_eq!(
            CmxError::MissingSkillMd {
                skill: "x".to_string()
            }
            .code(),
            "missing-skill-md"
        );

        assert_eq!(
            CmxError::VersionGuard {
                tool: "x".to_string()
            }
            .code(),
            "version-guard"
        );

        assert_eq!(
            CmxError::Drift {
                tool: "x".to_string()
            }
            .code(),
            "drift"
        );

        assert_eq!(
            CmxError::Git {
                operation: GitOp::Clone,
                stderr: "err".to_string()
            }
            .code(),
            "git"
        );

        assert_eq!(CmxError::Llm(LlmError::Other("x".to_string())).code(), "llm");
    }

    #[test]
    fn cmx_error_converts_to_anyhow() {
        // CmxError: Error + Send + Sync + 'static → automatically convertible to anyhow::Error
        let ce = CmxError::SourceNotFound {
            name: "x".to_string(),
        };
        let ae: anyhow::Error = ce.into();
        assert!(ae.to_string().contains("not found"));
    }
}
