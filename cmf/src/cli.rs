//! clap CLI definition (7 commands: facet, recipe, plugin, manifest,
//! marketplace, validate, status).

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cmf",
    about = "Publisher and authoring tool for context mixer facets, recipes, and plugins",
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
    /// Manage facets in the current repository
    Facet {
        /// The facet subcommand to run.
        #[command(subcommand)]
        action: FacetAction,
    },
    /// Manage recipes for assembling agents from facets
    Recipe {
        /// The recipe subcommand to run.
        #[command(subcommand)]
        action: RecipeAction,
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
    /// Show repository overview: plugins, facets, validation summary
    Status,
}

/// Subcommands for `cmf facet`.
#[derive(Subcommand)]
pub enum FacetAction {
    /// List facets in the current repository
    List,
    /// Check frontmatter, scope boundaries, and dependency constraints
    Validate,
}

/// Subcommands for `cmf recipe`.
#[derive(Subcommand)]
pub enum RecipeAction {
    /// List available recipes
    List,
    /// Assemble an agent from facets per recipe
    Assemble {
        /// Recipe name (omit when using --all)
        name: Option<String>,
        /// Assemble all recipes
        #[arg(long, conflicts_with = "name")]
        all: bool,
    },
    /// Show diff between assembled output and current agent
    Diff {
        /// Recipe name
        name: String,
    },
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
