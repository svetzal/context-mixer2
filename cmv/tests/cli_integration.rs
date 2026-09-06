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
        "--knowledge-base",
        "../kb",
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
                knowledge_base: Some(PathBuf::from("../kb")),
            },
        }
    );
}

#[test]
fn parses_status_with_location_and_json() {
    let cli = Cli::try_parse_from([
        "cmv",
        "status",
        "--json",
        "--root",
        "proj",
        "--knowledge-base",
        "kb",
    ])
    .expect("status should parse");
    assert_eq!(
        cli.command,
        Commands::Status {
            json: true,
            location: LocationArgs {
                root: Some(PathBuf::from("proj")),
                manifest: None,
                knowledge_base: Some(PathBuf::from("kb")),
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
fn explain_is_not_yet_a_command() {
    assert!(
        Cli::try_parse_from(["cmv", "explain", "some/key"]).is_err(),
        "explain lands in a later commit"
    );
}
