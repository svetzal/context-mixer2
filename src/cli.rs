use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cmx",
    about = "Package manager for curated agentic context — agents and skills",
    version
)]
pub struct Cli {
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
}

#[derive(Subcommand)]
pub enum SourceAction {
    /// Register a source repository (local path or git:url)
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
    /// Pull latest changes for a git-backed source
    Pull {
        /// Name of the source to pull
        name: String,
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
        name: String,
        /// Install into the current project instead of globally
        #[arg(long)]
        local: bool,
    },
    /// List installed artifacts
    List,
}
