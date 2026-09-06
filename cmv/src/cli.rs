//! Command-line grammar for `cmv check`, `cmv status`, and `cmv explain`.

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

/// Where the project, its manifest, and the knowledge base are, and which
/// tree of the knowledge base to verify.
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
    /// Overrides resolution through the cmx source registry and the
    /// `knowledge_base.path` the manifest recorded.
    #[arg(long, value_name = "PATH")]
    pub knowledge_base: Option<PathBuf>,
    /// Verify the knowledge base's working tree even when the manifest pins a
    /// revision its HEAD has moved past. For validator authors iterating on a
    /// knowledge base; the report says which tree was used.
    #[arg(long)]
    pub at_head: bool,
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
    /// knowledge base, an unreachable pinned revision, or bad usage.
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
    /// validator coverage without running any validator.
    Status {
        /// Emit the summary as JSON.
        #[arg(long)]
        json: bool,
        /// Where the project, manifest, and knowledge base are.
        #[command(flatten)]
        location: LocationArgs,
    },
    /// Show what `check` would do for one intent — how its record resolves,
    /// its validators, which would run and with what argv and config — without
    /// running anything.
    ///
    /// Exit 2 when the intent is neither compiled nor dropped in the manifest.
    Explain {
        /// Catalog key of the intent (e.g. `craftsperson/rust/put-gateways-at-effect-boundaries`),
        /// or its record id when the argument contains a `.`.
        #[arg(value_name = "INTENT")]
        intent: String,
        /// Emit the explanation as JSON.
        #[arg(long)]
        json: bool,
        /// Where the project, manifest, and knowledge base are.
        #[command(flatten)]
        location: LocationArgs,
    },
}
