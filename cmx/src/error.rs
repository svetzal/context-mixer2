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
    /// A hand-authored artifact of the given `kind` is already available from a
    /// registered source, so `cmx adopt` refuses it in favor of `install`.
    #[error(
        "'{name}' is available in a registered source — run `cmx {kind} install {name}` to \
         track it. (adopt is for hand-authored artifacts that no source provides.)"
    )]
    AdoptUntracked {
        /// The artifact's name.
        name: String,
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
    },

    /// The artifact is already tracked in the lock file, so there is nothing to adopt.
    #[error("'{name}' is already tracked — nothing to adopt.")]
    AdoptAlreadyTracked {
        /// The artifact's name.
        name: String,
    },

    /// The artifact is tracked but has drifted from its recorded checksum, so
    /// `adopt` (which only handles orphaned/untracked artifacts) does not apply.
    #[error(
        "'{name}' is tracked but locally modified (drifted), not orphaned — adopt does not yet \
         re-home drifted artifacts. Inspect with `cmx info {name}`."
    )]
    AdoptDrifted {
        /// The artifact's name.
        name: String,
    },

    /// The artifact is registered as externally managed, so it must be removed
    /// from the external list before cmx can adopt it.
    #[error(
        "'{name}' is marked external (managed by another tool) — remove it from the external \
         list (`cmx config external remove ...`) before adopting it with cmx."
    )]
    AdoptExternal {
        /// The artifact's name.
        name: String,
    },

    /// No file for the requested artifact exists on disk to adopt.
    #[error("No {kind} named '{name}' found on disk. Run `cmx doctor` to see what is adoptable.")]
    AdoptNotFoundOnDisk {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
    },

    // -----------------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------------
    /// The configured LLM gateway name is not one of the supported values.
    #[error("Unknown gateway '{value}'. Use 'openai' or 'ollama'.")]
    UnknownGateway {
        /// The unrecognized gateway value supplied by the user.
        value: String,
    },

    // -----------------------------------------------------------------------
    // Copy / install
    // -----------------------------------------------------------------------
    /// The given source path does not resolve to a usable artifact location.
    #[error("Invalid source path: {path}")]
    InvalidSourcePath {
        /// The offending path, as supplied by the user.
        path: String,
    },

    /// A skill install was rolled back because the required `SKILL.md` file
    /// was missing from the copied files.
    #[error("Skill '{name}' is missing SKILL.md. Partial install removed.")]
    MissingSkillMdOnInstall {
        /// The skill's name.
        name: String,
    },

    // -----------------------------------------------------------------------
    // LLM (feature-gated callers; variants are always compiled)
    // -----------------------------------------------------------------------
    /// `cmx diff`'s LLM-backed analysis was requested but no LLM client is configured.
    #[error("LLM client not configured for diff analysis")]
    LlmNotConfiguredDiff,

    /// An LLM-backed operation was requested but no LLM client is configured.
    #[error("LLM client not configured")]
    LlmNotConfigured,

    /// The LLM client returned a response with no usable summary text.
    #[error("the LLM returned an empty summary")]
    LlmEmptySummary,

    /// The file at `path` has no content that can be sent to the LLM for summarization
    /// (e.g. empty, binary, or unreadable).
    #[error("no readable content to summarize at {path}")]
    NoReadableContentToSummarize {
        /// The path that could not be read for summarization.
        path: String,
    },

    // -----------------------------------------------------------------------
    // Install / artifact
    // -----------------------------------------------------------------------
    /// The installed copy has been edited in place since it was installed, so
    /// installing over it would silently discard those edits without `--force`.
    #[error(
        "'{name}' has local modifications. Use --force to overwrite, \
         or 'cmx {kind} diff {name}' to review changes first."
    )]
    LocallyModified {
        /// The artifact's name.
        name: String,
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
    },

    /// The installed artifact's version is newer than the source's version,
    /// so installing would be a downgrade and requires `--force`.
    #[error(
        "Refusing to install '{name}': the installed copy is version {installed}, which is \
         newer than the source's {source_version}. Use --force to downgrade."
    )]
    InstalledNewerThanSource {
        /// The artifact's name.
        name: String,
        /// The version currently installed.
        installed: String,
        /// The version offered by the source.
        source_version: String,
    },

    /// No installed artifact of the requested kind matches the given name.
    #[error("No installed {kind} named '{name}' found. {hint}")]
    ArtifactNotInstalled {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
        /// A contextual suggestion for the user (e.g. how to list candidates).
        hint: String,
    },

    /// No installed artifact (of any kind) matches the given name.
    #[error("No installed artifact named '{name}' found. {hint}")]
    ArtifactNotFound {
        /// The artifact's name.
        name: String,
        /// A contextual suggestion for the user (e.g. how to list candidates).
        hint: String,
    },

    /// The lock file references the artifact, but no matching file exists on disk.
    #[error("No installed {kind} named '{name}' found on disk. {hint}")]
    ArtifactNotInstalledOnDisk {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
        /// A contextual suggestion for the user (e.g. how to list candidates).
        hint: String,
    },

    /// No artifact of the requested kind and name exists in any registered source.
    #[error("No {kind} named '{name}' found in registered sources.")]
    ArtifactNotInSources {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
    },

    /// No artifact of the requested kind and name exists in any registered
    /// source, with an additional hint for the user.
    #[error("No {kind} named '{name}' found in any registered source. {hint}")]
    ArtifactNotInAnySources {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
        /// A contextual suggestion for the user (e.g. how to add a source).
        hint: String,
    },

    // -----------------------------------------------------------------------
    // Sets
    // -----------------------------------------------------------------------
    /// No set with the given name exists.
    #[error("Set '{name}' not found.")]
    SetNotFound {
        /// The set's name.
        name: String,
    },

    /// A set with the given name already exists.
    #[error("Set '{name}' already exists.")]
    SetAlreadyExists {
        /// The set's name.
        name: String,
    },

    // -----------------------------------------------------------------------
    // Set members
    // -----------------------------------------------------------------------
    /// The `--from-plugin` value did not parse as `<source>:<plugin>`.
    #[error("Invalid --from-plugin value '{spec}'; expected <source>:<plugin>.")]
    InvalidFromPlugin {
        /// The raw, unparseable `--from-plugin` value.
        spec: String,
    },

    /// The named source has no marketplace manifest, so plugins cannot be
    /// resolved from it.
    #[error(
        "Source '{source_name}' has no marketplace (.claude-plugin/marketplace.json not found)."
    )]
    SourceNoMarketplace {
        /// The source's name.
        source_name: String,
    },

    /// The named plugin does not appear in the named source's marketplace manifest.
    #[error("Plugin '{plugin}' not found in marketplace of source '{source_name}'.")]
    PluginNotFound {
        /// The plugin's name.
        plugin: String,
        /// The source's name.
        source_name: String,
    },

    /// The plugin's marketplace entry points at a remote source type that cmx
    /// cannot yet resolve to seed set membership.
    #[error(
        "Plugin '{plugin}' uses remote source '{source_type}' which is not yet supported; \
         cannot seed a set from it."
    )]
    PluginRemoteUnsupported {
        /// The plugin's name.
        plugin: String,
        /// The unsupported remote source type declared by the plugin.
        source_type: String,
    },

    /// The artifact has no lock file entry, so its kind and source cannot be resolved.
    #[error(
        "Artifact '{name}' is not installed (no lockfile entry); cannot resolve its \
         kind/source. {hint}"
    )]
    ArtifactNotInLockfile {
        /// The artifact's name.
        name: String,
        /// A contextual suggestion for the user.
        hint: String,
    },

    /// The artifact has lock file entries, but none matches the requested kind.
    #[error("Artifact '{name}' has no lockfile entry matching the requested kind.")]
    ArtifactNoMatchingKind {
        /// The artifact's name.
        name: String,
    },

    /// The artifact's name matches both an agent and a skill; the caller must
    /// disambiguate with a `kind:name` prefix.
    #[error(
        "'{name}' is ambiguous across kinds — use skill:{name} or agent:{name} to disambiguate."
    )]
    ArtifactAmbiguousKind {
        /// The ambiguous artifact name.
        name: String,
    },

    // -----------------------------------------------------------------------
    // Source
    // -----------------------------------------------------------------------
    /// A source with the given name is already registered.
    #[error("Source '{name}' already exists. Remove it first to re-register.")]
    SourceAlreadyExists {
        /// The source's name.
        name: String,
    },

    /// The path given for a new source does not exist on disk.
    #[error("Source path {path} does not exist. {hint}")]
    SourcePathNotFound {
        /// The missing path.
        path: String,
        /// A contextual suggestion for the user.
        hint: &'static str,
    },

    /// The path given for a new source exists but is not a directory.
    #[error("'{path}' is not a directory.")]
    SourcePathNotDir {
        /// The offending path.
        path: String,
    },

    /// The target directory for cloning a new git source already exists.
    #[error("Clone directory {path} already exists. Remove it or choose a different name.")]
    CloneDirAlreadyExists {
        /// The already-existing clone directory path.
        path: String,
    },

    /// The clone directory recorded for a source no longer exists on disk.
    #[error("Clone directory {path} does not exist. Try removing and re-adding the source.")]
    CloneDirNotFound {
        /// The missing clone directory path.
        path: String,
    },

    // -----------------------------------------------------------------------
    // Source iter
    // -----------------------------------------------------------------------
    /// No sources are registered at all, so there is nothing to search.
    #[error("No sources registered. Add one with: cmx source add <name> <path-or-url>")]
    NoSourcesRegistered,

    /// The requested artifact name matches artifacts in more than one
    /// registered source, so the caller must disambiguate with `<source>:<name>`.
    #[error("'{name}' found in multiple sources: {sources}. Use <source>:{name} to disambiguate.")]
    AmbiguousArtifact {
        /// The ambiguous artifact name.
        name: String,
        /// A comma-separated list of the source names that provide it.
        sources: String,
    },

    // -----------------------------------------------------------------------
    // Source update
    // -----------------------------------------------------------------------
    /// No registered source has the given name.
    #[error("Source '{name}' not found.")]
    SourceNotFound {
        /// The source's name.
        name: String,
    },

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------
    /// The named platform has no installed copy of the skill to sync from.
    #[error("'{platform}' has no copy of this skill to sync from.")]
    SyncNoCopy {
        /// The platform with no copy to sync from.
        platform: String,
    },

    /// `cmx skill sync` was invoked for agents, which are reformatted per
    /// platform and are not yet supported by format-aware reconciliation.
    #[error(
        "`sync` currently supports skills only. Agents are reformatted per platform \
         (e.g. Codex TOML), so cross-platform agent reconciliation needs format-aware \
         handling (not yet implemented)."
    )]
    SyncAgentsNotSupported,

    /// The named skill is not installed on any managed platform, so there is
    /// nothing to sync.
    #[error("Skill '{name}' is not installed on any managed platform. {hint}")]
    SyncNotInstalled {
        /// The skill's name.
        name: String,
        /// A contextual suggestion for the user.
        hint: String,
    },

    // -----------------------------------------------------------------------
    // Uninstall
    // -----------------------------------------------------------------------
    /// No matching installed artifact was found in the given scope/platform to uninstall.
    #[error("No {kind} named '{name}' found in {scope} scope on {where_}. {hint}")]
    ArtifactNotFoundToUninstall {
        /// The kind of artifact (agent or skill).
        kind: ArtifactKind,
        /// The artifact's name.
        name: String,
        /// The install scope searched (e.g. "global" or "local").
        scope: String,
        /// A description of the platform(s) searched.
        where_: String,
        /// A contextual suggestion for the user.
        hint: String,
    },

    // -----------------------------------------------------------------------
    // Promote
    // -----------------------------------------------------------------------
    /// The active platform stores agents in a transformed format (e.g. Codex
    /// TOML), so it cannot serve as the source of a promotion back to the
    /// canonical markdown home.
    #[error(
        "Can't promote '{name}': the active platform stores agents as transformed TOML, not \
         the canonical markdown the home holds. Promote from a markdown platform (e.g. claude)."
    )]
    PromoteTomlTransformed {
        /// The artifact's name.
        name: String,
    },

    /// No installed copy of the artifact differs from the canonical home, so
    /// there are no in-place edits to promote.
    #[error(
        "No in-place edits detected on any platform — nothing to promote. The home already \
         differs from the installed copies (it was changed elsewhere). Run \
         `cmx skill update {name} --force` to pull the home over the installs, or \
         `cmx skill promote {name} --from <name>` to force a specific copy into the home."
    )]
    PromoteNoEdits {
        /// The artifact's name.
        name: String,
    },

    /// More than one installed copy has diverged from the canonical home in
    /// different ways, so cmx cannot automatically choose which to promote.
    #[error(
        "Multiple platforms have diverging in-place edits: {platforms}. cmx can't tell which \
         should become the canonical home copy. Inspect them with `cmx skill diff {name}`, then \
         promote the one you want with `cmx skill promote {name} --from <name>`."
    )]
    PromoteDiverging {
        /// The artifact's name.
        name: String,
        /// A comma-separated list of the platforms with diverging edits.
        platforms: String,
    },
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
