use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::platform::Platform;

#[derive(Parser)]
#[command(
    name = "cmx",
    about = "Package manager for curated agentic context — agents and skills",
    version
)]
pub struct Cli {
    /// Target AI coding assistant platform (env: `CMX_PLATFORM`)
    #[arg(long, value_enum, global = true, default_value_t = Platform::Claude, env = "CMX_PLATFORM")]
    pub platform: Platform,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage source repositories
    Source {
        #[command(subcommand)]
        action: SourceAction,
    },
    /// Manage agents
    Agent {
        #[command(subcommand)]
        action: ArtifactAction,
    },
    /// Manage skills
    Skill {
        #[command(subcommand)]
        action: ArtifactAction,
    },
    /// List all installed agents and skills
    List,
    /// Survey the whole system installation across every platform (read-only)
    Doctor {
        /// Also survey project (local) scope, not just global
        #[arg(long)]
        local: bool,
        /// Adopt every orphaned artifact into the canonical home (mutating)
        #[arg(long = "adopt-all")]
        adopt_all: bool,
        /// With --adopt-all, only adopt orphans under this install directory
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Manage the canonical home for hand-authored artifacts
    Home {
        #[command(subcommand)]
        action: HomeAction,
    },
    /// Show installed artifacts that have updates available
    Outdated,
    /// Search all sources for agents and skills by keyword
    Search {
        /// Keyword to search for in artifact names and descriptions
        query: String,
    },
    /// Show detailed metadata for an installed artifact
    Info {
        /// Artifact name
        name: String,
    },
    /// View or modify cmx configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum SourceAction {
    /// Register a source repository (local path or git URL)
    Add {
        /// Name to identify this source
        name: String,
        /// Local path or URL to the source repository
        path_or_url: String,
    },
    /// List registered sources
    List,
    /// Show available agents and skills in a source
    Browse {
        /// Name of the source to browse
        name: String,
    },
    /// Fetch latest changes for git-backed sources
    Update {
        /// Name of a specific source to update (default: all)
        name: Option<String>,
    },
    /// Unregister a source (does not delete artifacts)
    Remove {
        /// Name of the source to remove
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ArtifactAction {
    /// Install an artifact from a source
    Install {
        /// Artifact name, or source:name to pin to a specific source
        name: Option<String>,
        /// Install all available artifacts from sources
        #[arg(long, conflicts_with = "name")]
        all: bool,
        /// Install into the current project instead of globally
        #[arg(long)]
        local: bool,
        /// Force overwrite even if locally modified
        #[arg(long)]
        force: bool,
    },
    /// List installed artifacts
    List,
    #[cfg(feature = "llm")]
    /// Compare installed artifact with source version using LLM analysis
    Diff {
        /// Artifact name
        name: String,
    },
    /// Update an installed artifact from its source
    Update {
        /// Artifact name
        name: Option<String>,
        /// Update all tracked artifacts
        #[arg(long, conflicts_with = "name")]
        all: bool,
        /// Force overwrite even if locally modified
        #[arg(long)]
        force: bool,
    },
    /// Uninstall installed artifact(s) — removed everywhere cmx tracks them
    Uninstall {
        /// Artifact name(s) to uninstall
        names: Vec<String>,
        /// Uninstall from project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// Adopt orphaned, hand-authored artifacts into the canonical home
    Adopt {
        /// Artifact name(s) to adopt (each must be an orphan reported by `cmx doctor`)
        names: Vec<String>,
        /// Adopt all orphans of this kind instead of named ones
        #[arg(long, conflicts_with = "names")]
        all: bool,
        /// With --all, only adopt orphans under this install directory
        #[arg(long)]
        from: Option<PathBuf>,
        /// Search project (local) scope as well as global for orphans
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand)]
pub enum HomeAction {
    /// Create the canonical home directory and register it as the `home` source
    Init,
    /// Print the resolved canonical home directory
    Path,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set LLM gateway (openai or ollama)
    Gateway {
        /// Gateway type: openai or ollama
        value: String,
    },
    /// Set LLM model name
    Model {
        /// Model name (e.g. gpt-5.4, qwen3.5:27b)
        value: String,
    },
    /// Manage `external` rules — artifacts another tool manages, which `doctor`
    /// reports as external instead of flagging
    External {
        #[command(subcommand)]
        action: ExternalAction,
    },
}

#[derive(Subcommand)]
pub enum ExternalAction {
    /// List the configured external rules
    List,
    /// Add an external rule: a directory (e.g. ~/.hermes/skills) or an artifact name
    Add {
        /// Directory path or bare artifact name to mark external
        entry: String,
    },
    /// Remove an external rule
    Remove {
        /// The directory path or name to remove
        entry: String,
    },
}
