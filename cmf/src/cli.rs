use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cmf",
    about = "Publisher and authoring tool for context mixer facets, recipes, and plugins",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage facets in the current repository
    Facet {
        #[command(subcommand)]
        action: FacetAction,
    },
    /// Manage recipes for assembling agents from facets
    Recipe {
        #[command(subcommand)]
        action: RecipeAction,
    },
    /// Manage plugins for the marketplace
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Generate multi-platform manifests
    Manifest {
        #[command(subcommand)]
        action: ManifestAction,
    },
    /// Validate and generate marketplace metadata
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Run all validation checks
    Validate,
    /// Show repository overview: plugins, facets, validation summary
    Status,
}

#[derive(Subcommand)]
pub enum FacetAction {
    /// List facets in the current repository
    List,
    /// Check frontmatter, scope boundaries, and dependency constraints
    Validate,
}

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

#[derive(Subcommand)]
pub enum ManifestAction {
    /// Generate multi-platform manifests (.claude-plugin, .codex-plugin, .cursor-plugin, gemini-extension.json)
    Generate,
}

#[derive(Subcommand)]
pub enum MarketplaceAction {
    /// Validate marketplace.json against actual plugins
    Validate,
    /// Generate marketplace.json from plugin directory structure
    Generate,
}
