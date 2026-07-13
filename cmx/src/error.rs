//! Typed, matchable domain errors for the `cmx` CLI layer.
//!
//! All fallible command-core APIs return [`Result<T>`].  The dispatch layer
//! (`cmx/src/dispatch/` and `cmx/src/main.rs`) continues to use `anyhow`;
//! `CliError` converts automatically via `anyhow`'s blanket `From<Error>`
//! implementation.
//!
//! # Design
//!
//! - [`CliError::Core`] is a transparent pass-through for
//!   [`cmx_core::error::CmxError`], so cmx-core errors flow through without
//!   wrapping.
//! - [`CliError::Message`] is a dynamic-string escape hatch for messages that
//!   are built at runtime (e.g. `non_home_guidance`, sync ambiguity text).
//! - All other variants carry stable, matchable domain information and produce
//!   Display strings that are **byte-identical** to the `bail!`/`anyhow!`
//!   strings they replaced, so existing substring-assertion tests continue to
//!   pass unchanged.

use crate::types::ArtifactKind;

// ---------------------------------------------------------------------------
// CliError
// ---------------------------------------------------------------------------

/// Typed domain errors produced by `cmx`'s command-core modules.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    // -----------------------------------------------------------------------
    // cmx-core pass-through
    // -----------------------------------------------------------------------
    /// Transparent wrapper for any error originating in `cmx-core`.
    #[error(transparent)]
    Core(#[from] cmx_core::error::CmxError),

    // -----------------------------------------------------------------------
    // Dynamic escape hatch
    // -----------------------------------------------------------------------
    /// A fully-formatted message built at runtime.
    ///
    /// Used for messages that require non-trivial formatting (e.g.
    /// `non_home_guidance`, sync ambiguity notes) where a dedicated variant
    /// would not add value.
    #[error("{0}")]
    Message(String),

    // -----------------------------------------------------------------------
    // Adopt
    // -----------------------------------------------------------------------
    #[error(
        "'{name}' is available in a registered source — run `cmx {kind} install {name}` to \
         track it. (adopt is for hand-authored artifacts that no source provides.)"
    )]
    AdoptUntracked { name: String, kind: ArtifactKind },

    #[error("'{name}' is already tracked — nothing to adopt.")]
    AdoptAlreadyTracked { name: String },

    #[error(
        "'{name}' is tracked but locally modified (drifted), not orphaned — adopt does not yet \
         re-home drifted artifacts. Inspect with `cmx info {name}`."
    )]
    AdoptDrifted { name: String },

    #[error(
        "'{name}' is marked external (managed by another tool) — remove it from the external \
         list (`cmx config external remove ...`) before adopting it with cmx."
    )]
    AdoptExternal { name: String },

    #[error("No {kind} named '{name}' found on disk. Run `cmx doctor` to see what is adoptable.")]
    AdoptNotFoundOnDisk { kind: ArtifactKind, name: String },

    // -----------------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------------
    #[error("Unknown gateway '{value}'. Use 'openai' or 'ollama'.")]
    UnknownGateway { value: String },

    // -----------------------------------------------------------------------
    // Copy / install
    // -----------------------------------------------------------------------
    #[error("Invalid source path: {path}")]
    InvalidSourcePath { path: String },

    #[error("Skill '{name}' is missing SKILL.md. Partial install removed.")]
    MissingSkillMdOnInstall { name: String },

    // -----------------------------------------------------------------------
    // LLM (feature-gated callers; variants are always compiled)
    // -----------------------------------------------------------------------
    #[error("LLM client not configured for diff analysis")]
    LlmNotConfiguredDiff,

    #[error("LLM client not configured")]
    LlmNotConfigured,

    #[error("the LLM returned an empty summary")]
    LlmEmptySummary,

    #[error("no readable content to summarize at {path}")]
    NoReadableContentToSummarize { path: String },

    // -----------------------------------------------------------------------
    // Install / artifact
    // -----------------------------------------------------------------------
    #[error(
        "'{name}' has local modifications. Use --force to overwrite, \
         or 'cmx {kind} diff {name}' to review changes first."
    )]
    LocallyModified { name: String, kind: ArtifactKind },

    #[error("No installed {kind} named '{name}' found. {hint}")]
    ArtifactNotInstalled {
        kind: ArtifactKind,
        name: String,
        hint: String,
    },

    #[error("No installed artifact named '{name}' found. {hint}")]
    ArtifactNotFound { name: String, hint: String },

    #[error("No installed {kind} named '{name}' found on disk. {hint}")]
    ArtifactNotInstalledOnDisk {
        kind: ArtifactKind,
        name: String,
        hint: String,
    },

    #[error("No {kind} named '{name}' found in registered sources.")]
    ArtifactNotInSources { kind: ArtifactKind, name: String },

    #[error("No {kind} named '{name}' found in any registered source. {hint}")]
    ArtifactNotInAnySources {
        kind: ArtifactKind,
        name: String,
        hint: String,
    },

    // -----------------------------------------------------------------------
    // Sets
    // -----------------------------------------------------------------------
    #[error("Set '{name}' not found.")]
    SetNotFound { name: String },

    #[error("Set '{name}' already exists.")]
    SetAlreadyExists { name: String },

    // -----------------------------------------------------------------------
    // Set members
    // -----------------------------------------------------------------------
    #[error("Invalid --from-plugin value '{spec}'; expected <source>:<plugin>.")]
    InvalidFromPlugin { spec: String },

    #[error(
        "Source '{source_name}' has no marketplace (.claude-plugin/marketplace.json not found)."
    )]
    SourceNoMarketplace { source_name: String },

    #[error("Plugin '{plugin}' not found in marketplace of source '{source_name}'.")]
    PluginNotFound { plugin: String, source_name: String },

    #[error(
        "Plugin '{plugin}' uses remote source '{source_type}' which is not yet supported; \
         cannot seed a set from it."
    )]
    PluginRemoteUnsupported { plugin: String, source_type: String },

    #[error(
        "Artifact '{name}' is not installed (no lockfile entry); cannot resolve its \
         kind/source. {hint}"
    )]
    ArtifactNotInLockfile { name: String, hint: String },

    #[error("Artifact '{name}' has no lockfile entry matching the requested kind.")]
    ArtifactNoMatchingKind { name: String },

    #[error(
        "'{name}' is ambiguous across kinds — use skill:{name} or agent:{name} to disambiguate."
    )]
    ArtifactAmbiguousKind { name: String },

    // -----------------------------------------------------------------------
    // Source
    // -----------------------------------------------------------------------
    #[error("Source '{name}' already exists. Remove it first to re-register.")]
    SourceAlreadyExists { name: String },

    #[error("Source path {path} does not exist. {hint}")]
    SourcePathNotFound { path: String, hint: &'static str },

    #[error("'{path}' is not a directory.")]
    SourcePathNotDir { path: String },

    #[error("Clone directory {path} already exists. Remove it or choose a different name.")]
    CloneDirAlreadyExists { path: String },

    #[error("Clone directory {path} does not exist. Try removing and re-adding the source.")]
    CloneDirNotFound { path: String },

    // -----------------------------------------------------------------------
    // Source iter
    // -----------------------------------------------------------------------
    #[error("No sources registered. Add one with: cmx source add <name> <path-or-url>")]
    NoSourcesRegistered,

    #[error("'{name}' found in multiple sources: {sources}. Use <source>:{name} to disambiguate.")]
    AmbiguousArtifact { name: String, sources: String },

    // -----------------------------------------------------------------------
    // Source update
    // -----------------------------------------------------------------------
    #[error("Source '{name}' not found.")]
    SourceNotFound { name: String },

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------
    #[error("'{platform}' has no copy of this skill to sync from.")]
    SyncNoCopy { platform: String },

    #[error(
        "`sync` currently supports skills only. Agents are reformatted per platform \
         (e.g. Codex TOML), so cross-platform agent reconciliation needs format-aware \
         handling (not yet implemented)."
    )]
    SyncAgentsNotSupported,

    #[error("Skill '{name}' is not installed on any managed platform. {hint}")]
    SyncNotInstalled { name: String, hint: String },

    // -----------------------------------------------------------------------
    // Uninstall
    // -----------------------------------------------------------------------
    #[error("No {kind} named '{name}' found in {scope} scope on {where_}. {hint}")]
    ArtifactNotFoundToUninstall {
        kind: ArtifactKind,
        name: String,
        scope: String,
        where_: String,
        hint: String,
    },

    // -----------------------------------------------------------------------
    // Promote
    // -----------------------------------------------------------------------
    #[error(
        "Can't promote '{name}': the active platform stores agents as transformed TOML, not \
         the canonical markdown the home holds. Promote from a markdown platform (e.g. claude)."
    )]
    PromoteTomlTransformed { name: String },

    #[error(
        "No in-place edits detected on any platform — nothing to promote. The home already \
         differs from the installed copies (it was changed elsewhere). Run \
         `cmx skill update {name} --force` to pull the home over the installs, or \
         `cmx skill promote {name} --from <name>` to force a specific copy into the home."
    )]
    PromoteNoEdits { name: String },

    #[error(
        "Multiple platforms have diverging in-place edits: {platforms}. cmx can't tell which \
         should become the canonical home copy. Inspect them with `cmx skill diff {name}`, then \
         promote the one you want with `cmx skill promote {name} --from <name>`."
    )]
    PromoteDiverging { name: String, platforms: String },
}

// ---------------------------------------------------------------------------
// Result alias
// ---------------------------------------------------------------------------

/// `cmx` CLI layer's canonical result type.
pub type Result<T> = core::result::Result<T, CliError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::error::CmxError;

    #[test]
    fn core_variant_delegates_display() {
        let ce = CmxError::SourceNotFound {
            name: "missing".to_string(),
        };
        let cli: CliError = ce.into();
        assert_eq!(cli.to_string(), "Source 'missing' not found.");
    }

    #[test]
    fn message_variant_passes_through() {
        let e = CliError::Message("dynamic error text".to_string());
        assert_eq!(e.to_string(), "dynamic error text");
    }

    #[test]
    fn set_not_found_formats_correctly() {
        let e = CliError::SetNotFound {
            name: "my-set".to_string(),
        };
        assert_eq!(e.to_string(), "Set 'my-set' not found.");
    }

    #[test]
    fn no_sources_registered_formats_correctly() {
        let e = CliError::NoSourcesRegistered;
        assert!(e.to_string().contains("cmx source add"));
    }

    #[test]
    fn cli_error_converts_to_anyhow() {
        let ce = CliError::SetNotFound {
            name: "x".to_string(),
        };
        let ae: anyhow::Error = ce.into();
        assert!(ae.to_string().contains("not found"));
    }
}
