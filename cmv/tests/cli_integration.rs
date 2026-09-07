//! Command grammar integration tests.

use std::path::PathBuf;

use clap::Parser;
use cmv::cli::{Cli, Commands, LocationArgs};

#[test]
fn parses_bare_check_with_defaults() {
    let cli = Cli::try_parse_from(["cmv", "check"]).expect("check should parse");
    assert_eq!(
        cli.command,
        Commands::Check {
            json: false,
            strict: false,
            location: LocationArgs::default(),
        }
    );
}

#[test]
fn parses_check_with_every_flag() {
    let cli = Cli::try_parse_from([
        "cmv",
        "check",
        "--json",
        "--strict",
        "--root",
        "proj",
        "--manifest",
        "out/manifest.json",
        "--atlas",
        "../kb",
        "--at-head",
    ])
    .expect("check should parse");
    assert_eq!(
        cli.command,
        Commands::Check {
            json: true,
            strict: true,
            location: LocationArgs {
                root: Some(PathBuf::from("proj")),
                manifest: Some(PathBuf::from("out/manifest.json")),
                atlas: Some(PathBuf::from("../kb")),
                at_head: true,
            },
        }
    );
}

#[test]
fn parses_status_with_location_and_json() {
    let cli = Cli::try_parse_from(["cmv", "status", "--json", "--root", "proj", "--atlas", "kb"])
        .expect("status should parse");
    assert_eq!(
        cli.command,
        Commands::Status {
            json: true,
            location: LocationArgs {
                root: Some(PathBuf::from("proj")),
                manifest: None,
                atlas: Some(PathBuf::from("kb")),
                at_head: false,
            },
        }
    );
}

#[test]
fn status_accepts_at_head() {
    let cli = Cli::try_parse_from(["cmv", "status", "--at-head"]).expect("status should parse");
    assert_eq!(
        cli.command,
        Commands::Status {
            json: false,
            location: LocationArgs {
                at_head: true,
                ..LocationArgs::default()
            },
        }
    );
}

#[test]
fn status_has_no_strict_flag() {
    assert!(Cli::try_parse_from(["cmv", "status", "--strict"]).is_err());
}

#[test]
fn a_subcommand_is_required() {
    assert!(Cli::try_parse_from(["cmv"]).is_err());
    assert!(Cli::try_parse_from(["cmv", "--json"]).is_err());
}

#[test]
fn parses_explain_with_an_intent_and_every_flag() {
    let cli = Cli::try_parse_from([
        "cmv",
        "explain",
        "craftsperson/rust/put-gateways-at-effect-boundaries",
        "--json",
        "--root",
        "proj",
        "--atlas",
        "kb",
        "--at-head",
    ])
    .expect("explain should parse");
    assert_eq!(
        cli.command,
        Commands::Explain {
            intent: "craftsperson/rust/put-gateways-at-effect-boundaries".to_string(),
            json: true,
            location: LocationArgs {
                root: Some(PathBuf::from("proj")),
                manifest: None,
                atlas: Some(PathBuf::from("kb")),
                at_head: true,
            },
        }
    );
}

#[test]
fn explain_requires_an_intent_and_has_no_strict_flag() {
    assert!(Cli::try_parse_from(["cmv", "explain"]).is_err());
    assert!(Cli::try_parse_from(["cmv", "explain", "some/key", "--strict"]).is_err());
}

#[test]
fn knowledge_base_is_a_hidden_alias_for_atlas() {
    let cli = Cli::try_parse_from(["cmv", "check", "--knowledge-base", "../kb"])
        .expect("the old spelling should still parse");
    assert_eq!(
        cli.command,
        Commands::Check {
            json: false,
            strict: false,
            location: LocationArgs {
                atlas: Some(PathBuf::from("../kb")),
                ..LocationArgs::default()
            },
        }
    );
    let help = match Cli::try_parse_from(["cmv", "check", "--help"]) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("--help exits early"),
    };
    assert!(help.contains("--atlas <PATH>"), "{help}");
    assert!(!help.contains("--knowledge-base"), "the alias must stay out of help:\n{help}");
}
