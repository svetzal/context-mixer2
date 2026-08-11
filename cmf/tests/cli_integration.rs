//! Integration tests for cmf's CLI argument parsing.

use clap::Parser;
use cmf::cli::{Cli, Commands, IntentAction, ManifestAction, MarketplaceAction, PluginAction};

#[test]
fn parse_intent_list() {
    let cli = Cli::try_parse_from(["cmf", "intent", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Intent {
            action: IntentAction::List
        }
    ));
}

#[test]
fn parse_intent_validate() {
    let cli = Cli::try_parse_from(["cmf", "intent", "validate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Intent {
            action: IntentAction::Validate
        }
    ));
}

#[test]
fn facet_and_recipe_commands_are_gone() {
    assert!(Cli::try_parse_from(["cmf", "facet", "list"]).is_err());
    assert!(Cli::try_parse_from(["cmf", "recipe", "list"]).is_err());
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
