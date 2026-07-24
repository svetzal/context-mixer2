//! Integration tests for cmf's CLI argument parsing.

use clap::Parser;
use cmf::cli::{
    Cli, Commands, FacetAction, ManifestAction, MarketplaceAction, PluginAction, RecipeAction,
};

#[test]
fn parse_facet_list() {
    let cli = Cli::try_parse_from(["cmf", "facet", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Facet {
            action: FacetAction::List
        }
    ));
}

#[test]
fn parse_facet_validate() {
    let cli = Cli::try_parse_from(["cmf", "facet", "validate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Facet {
            action: FacetAction::Validate
        }
    ));
}

#[test]
fn parse_recipe_list() {
    let cli = Cli::try_parse_from(["cmf", "recipe", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Recipe {
            action: RecipeAction::List
        }
    ));
}

#[test]
fn parse_recipe_assemble_all() {
    let cli = Cli::try_parse_from(["cmf", "recipe", "assemble", "--all"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Recipe {
            action: RecipeAction::Assemble { all: true, .. }
        }
    ));
}

#[test]
fn parse_recipe_assemble_named() {
    let cli = Cli::try_parse_from(["cmf", "recipe", "assemble", "myrecipe"]).unwrap();
    match cli.command {
        Commands::Recipe {
            action: RecipeAction::Assemble { name, .. },
        } => {
            assert_eq!(name, Some("myrecipe".to_string()));
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parse_recipe_diff() {
    let cli = Cli::try_parse_from(["cmf", "recipe", "diff", "myrecipe"]).unwrap();
    match cli.command {
        Commands::Recipe {
            action: RecipeAction::Diff { name },
        } => {
            assert_eq!(name, "myrecipe");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parse_plugin_init() {
    let cli = Cli::try_parse_from(["cmf", "plugin", "init", "myplugin"]).unwrap();
    match cli.command {
        Commands::Plugin {
            action: PluginAction::Init { name },
        } => {
            assert_eq!(name, "myplugin");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parse_plugin_validate() {
    let cli = Cli::try_parse_from(["cmf", "plugin", "validate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Plugin {
            action: PluginAction::Validate
        }
    ));
}

#[test]
fn parse_plugin_list() {
    let cli = Cli::try_parse_from(["cmf", "plugin", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Plugin {
            action: PluginAction::List
        }
    ));
}

#[test]
fn parse_manifest_generate() {
    let cli = Cli::try_parse_from(["cmf", "manifest", "generate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Manifest {
            action: ManifestAction::Generate
        }
    ));
}

#[test]
fn parse_marketplace_validate() {
    let cli = Cli::try_parse_from(["cmf", "marketplace", "validate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Marketplace {
            action: MarketplaceAction::Validate
        }
    ));
}

#[test]
fn parse_marketplace_generate() {
    let cli = Cli::try_parse_from(["cmf", "marketplace", "generate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Marketplace {
            action: MarketplaceAction::Generate
        }
    ));
}

#[test]
fn parse_validate() {
    let cli = Cli::try_parse_from(["cmf", "validate"]).unwrap();
    assert!(matches!(cli.command, Commands::Validate));
}

#[test]
fn parse_status() {
    let cli = Cli::try_parse_from(["cmf", "status"]).unwrap();
    assert!(matches!(cli.command, Commands::Status));
}

#[test]
fn parse_invalid_command_errors() {
    assert!(Cli::try_parse_from(["cmf", "notacommand"]).is_err());
}
