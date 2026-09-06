//! Command grammar integration tests.

use std::path::PathBuf;

use clap::Parser;
use cmf::cli::{Cli, Commands, SurfaceArg};

#[test]
fn parses_assemble_with_explanation() {
    let cli = Cli::try_parse_from(["cmf", "assemble", "dependency-change", "--explain"])
        .expect("assemble should parse");
    assert!(matches!(cli.command, Commands::Assemble { explain: true, .. }));
}

#[test]
fn parses_assemble_with_manifest_path() {
    let cli = Cli::try_parse_from([
        "cmf",
        "assemble",
        "dependency-change",
        "--manifest",
        "out/cmf-manifest.json",
    ])
    .expect("assemble should parse");
    match cli.command {
        Commands::Assemble { manifest, .. } => {
            assert_eq!(manifest, Some(PathBuf::from("out/cmf-manifest.json")));
        }
        Commands::Install { .. } | Commands::Status => panic!("expected assemble"),
    }
}

#[test]
fn assemble_without_manifest_flag_writes_none() {
    let cli = Cli::try_parse_from(["cmf", "assemble", "dependency-change"])
        .expect("assemble should parse");
    assert!(matches!(cli.command, Commands::Assemble { manifest: None, .. }));
}

#[test]
fn parses_install_preview_and_apply_controls() {
    let cli = Cli::try_parse_from([
        "cmf",
        "install",
        "dependency-change",
        "--surface",
        "agent",
        "--local",
        "--apply",
        "--force",
    ])
    .expect("install should parse");
    assert!(matches!(
        cli.command,
        Commands::Install {
            surface: Some(SurfaceArg::Agent),
            local: true,
            apply: true,
            force: true,
            ..
        }
    ));
}

#[test]
fn legacy_publisher_commands_are_gone() {
    for command in ["intent", "plugin", "manifest", "marketplace", "validate"] {
        assert!(Cli::try_parse_from(["cmf", command]).is_err(), "{command} must stay removed");
    }
}
