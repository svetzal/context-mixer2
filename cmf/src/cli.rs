//! Command-line grammar for guidance materialization.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "cmf",
    about = "Assemble intent records into installable agent guidance",
    version
)]
/// Top-level `cmf` parser.
pub struct Cli {
    /// Intent knowledge-base root. Defaults to the current directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Commands,
}

/// Delivery surface for assembled guidance.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SurfaceArg {
    /// Always-loaded or explicitly selected custom agent guidance.
    Agent,
    /// Triggered, situational skill guidance.
    Skill,
}

/// Supported `cmf` operations.
#[derive(Subcommand)]
pub enum Commands {
    /// Assemble a profile and write the artifact to stdout.
    Assemble {
        /// Profile path, or a name resolved below `<root>/profiles/`.
        profile: PathBuf,
        /// Override the profile's delivery surface.
        #[arg(long, value_enum)]
        surface: Option<SurfaceArg>,
        /// Write selection and traversal provenance to stderr.
        #[arg(long)]
        explain: bool,
    },
    /// Preview or apply installation of an assembled profile.
    Install {
        /// Profile path, or a name resolved below `<root>/profiles/`.
        profile: PathBuf,
        /// Override the profile's delivery surface.
        #[arg(long, value_enum)]
        surface: Option<SurfaceArg>,
        /// Install project-locally instead of user-wide.
        #[arg(long)]
        local: bool,
        /// Apply the displayed plan.
        #[arg(long)]
        apply: bool,
        /// Overwrite drifted or newer installed guidance.
        #[arg(long)]
        force: bool,
    },
    /// Report intent and profile inventory for the knowledge base.
    Status,
}
