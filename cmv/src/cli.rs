//! Command-line grammar for `cmv check` and `cmv status`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cmv",
    about = "Verify that a project holds the intents cmf compiled for it",
    version
)]
/// Top-level `cmv` parser.
pub struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Commands,
}

/// Where the project, its manifest, and the knowledge base are.
#[derive(Args, Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationArgs {
    /// Project root. Defaults to the current directory.
    #[arg(long, value_name = "PROJECT")]
    pub root: Option<PathBuf>,
    /// Compile manifest to verify against. Defaults to
    /// `<root>/.context-mixer/cmf-manifest.json`, where `cmf install --local`
    /// writes it.
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,
    /// Knowledge-base root holding the intent records and validators.
    /// Defaults to the `knowledge_base.path` the manifest recorded.
    #[arg(long, value_name = "PATH")]
    pub knowledge_base: Option<PathBuf>,
}

/// Supported `cmv` operations.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Commands {
    /// Run every compiled intent's validators and exit nonzero when a required
    /// intent is not held.
    ///
    /// Exit 0 when every required, applicable validator passed; 1 when any
    /// required validator failed (or, with --strict, any intent was
    /// unchecked); 2 for a missing or malformed manifest, an unreadable
    /// knowledge base, or bad usage.
    Check {
        /// Emit the full per-intent report as JSON instead of the listing.
        #[arg(long)]
        json: bool,
        /// Also fail the run when any intent could not be checked.
        #[arg(long)]
        strict: bool,
        /// Where the project, manifest, and knowledge base are.
        #[command(flatten)]
        location: LocationArgs,
    },
    /// Summarize the manifest, the knowledge-base pin, detected languages, and
    /// validator coverage without running anything.
    Status {
        /// Emit the summary as JSON.
        #[arg(long)]
        json: bool,
        /// Where the project, manifest, and knowledge base are.
        #[command(flatten)]
        location: LocationArgs,
    },
}
