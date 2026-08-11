//! clap CLI definition for intent authoring and artifact publishing.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cmf",
    about = "Compiler and publisher for intent-based agentic guidance",
    version
)]
/// Top-level `cmf` command-line parser.
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Commands,
}

/// The top-level `cmf` subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Inspect and validate elemental intent records
    Intent {
        /// The intent subcommand to run.
        #[command(subcommand)]
        action: IntentAction,
    },
    /// Manage plugins for the marketplace
    Plugin {
        /// The plugin subcommand to run.
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Generate multi-platform manifests
    Manifest {
        /// The manifest subcommand to run.
        #[command(subcommand)]
        action: ManifestAction,
    },
    /// Validate and generate marketplace metadata
    Marketplace {
        /// The marketplace subcommand to run.
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Run all validation checks
    Validate,
    /// Show repository overview: intents, plugins, and validation summary
    Status,
}

/// Subcommands for `cmf intent`.
#[derive(Subcommand)]
pub enum IntentAction {
    /// List intents in the current repository
    List,
    /// Check intent schemas, identities, and graph relationships
    Validate,
}

/// Subcommands for `cmf plugin`.
#[derive(Subcommand)]
pub enum PluginAction {
    /// Scaffold a new plugin directory (plugin.json + agents/ + skills/)
    Init {
        /// Plugin name
        name: String,
    },
    /// Validate plugin structure
    Validate,
    /// List plugins in the current marketplace repository
    List,
}

/// Subcommands for `cmf manifest`.
#[derive(Subcommand)]
pub enum ManifestAction {
    /// Generate multi-platform manifests (.claude-plugin, .copilot-plugin, .cursor-plugin, .windsurf-plugin, .gemini-plugin)
    Generate,
}

/// Subcommands for `cmf marketplace`.
#[derive(Subcommand)]
pub enum MarketplaceAction {
    /// Validate marketplace.json against actual plugins
    Validate,
    /// Generate marketplace.json from plugin directory structure
    Generate,
}
