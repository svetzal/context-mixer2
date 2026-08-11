//! Command grammar integration tests.

use clap::Parser;
use cmf::cli::{Cli, Commands, SurfaceArg};

#[test]
fn parses_assemble_with_explanation() {
    let cli = Cli::try_parse_from(["cmf", "assemble", "dependency-change", "--explain"])
        .expect("assemble should parse");
    assert!(matches!(cli.command, Commands::Assemble { explain: true, .. }));
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
