//! Binary entry point for the `cmf` CLI: dispatches CLI commands (including
//! status).

use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cmx::gateway::{Filesystem, RealFilesystem};

use cmf::display::status_report;
use cmf::intent::{IntentList, scan_intents, validate_intents};
use cmf::manifest::{ManifestSummary, generate_manifests};
use cmf::marketplace::{generate_marketplace, validate_marketplace};
use cmf::plugin::{PluginList, init_plugin, scan_plugins, validate_all_plugins};
use cmf::repo::{RepoRoot, detect_repo};
use cmf::validate::validate_all;
use cmf::validation::ValidationReport;

use cmf::cli::{Cli, Commands, IntentAction, ManifestAction, MarketplaceAction, PluginAction};

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let fs = RealFilesystem;
    let cwd = env::current_dir()?;
    let root = detect_repo(&cwd, &fs)?;

    run(cli, &root, &fs)
}

/// Print a validation report and map it to an exit code: `2` when it carries any
/// error-level issue (so CI can gate on it), `SUCCESS` otherwise.
fn report_and_exit(report: &ValidationReport) -> ExitCode {
    print!("{report}");
    if report.has_errors() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

fn run(cli: Cli, root: &RepoRoot, fs: &dyn Filesystem) -> Result<ExitCode> {
    match cli.command {
        Commands::Intent { action } => handle_intent(&action, root, fs),
        Commands::Plugin { action } => handle_plugin(&action, root, fs),
        Commands::Manifest { action } => handle_manifest(&action, root, fs),
        Commands::Marketplace { action } => handle_marketplace(&action, root, fs),
        Commands::Validate => {
            let issues = validate_all(root, fs)?;
            Ok(report_and_exit(&ValidationReport(issues)))
        }
        Commands::Status => {
            print!("{}", status_report(root, fs));
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn handle_intent(action: &IntentAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<ExitCode> {
    match action {
        IntentAction::List => {
            let intents = scan_intents(root, fs)?;
            print!("{}", IntentList(intents));
            Ok(ExitCode::SUCCESS)
        }
        IntentAction::Validate => {
            let issues = validate_intents(root, fs)?;
            Ok(report_and_exit(&ValidationReport(issues)))
        }
    }
}

fn handle_plugin(action: &PluginAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<ExitCode> {
    match action {
        PluginAction::Init { name } => {
            let path = init_plugin(root, name, fs)?;
            println!("Created plugin '{name}' at {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        PluginAction::Validate => {
            let issues = validate_all_plugins(root, fs)?;
            Ok(report_and_exit(&ValidationReport(issues)))
        }
        PluginAction::List => {
            let plugins = scan_plugins(root, fs)?;
            print!("{}", PluginList(plugins));
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn handle_manifest(
    action: &ManifestAction,
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Result<ExitCode> {
    match action {
        ManifestAction::Generate => {
            let written = generate_manifests(root, fs)?;
            print!("{}", ManifestSummary(written));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_marketplace(
    action: &MarketplaceAction,
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Result<ExitCode> {
    match action {
        MarketplaceAction::Validate => {
            let issues = validate_marketplace(root, fs)?;
            Ok(report_and_exit(&ValidationReport(issues)))
        }
        MarketplaceAction::Generate => {
            let count = generate_marketplace(root, fs)?;
            println!("Generated marketplace.json with {count} plugins");
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmf::repo::{RepoKind, RepoRoot};
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    fn unknown_root() -> RepoRoot {
        RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Unknown,
            has_intents: false,
            has_plugins_dir: false,
        }
    }

    #[test]
    fn handle_intent_list_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_intent(&IntentAction::List, &root, &fs).is_ok());
    }

    #[test]
    fn handle_intent_validate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_intent(&IntentAction::Validate, &root, &fs).is_ok());
    }

    #[test]
    fn handle_plugin_list_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_plugin(&PluginAction::List, &root, &fs).is_ok());
    }

    #[test]
    fn handle_plugin_validate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_plugin(&PluginAction::Validate, &root, &fs).is_ok());
    }

    #[test]
    fn handle_manifest_generate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_manifest(&ManifestAction::Generate, &root, &fs).is_ok());
    }

    #[test]
    fn handle_marketplace_validate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_marketplace(&MarketplaceAction::Validate, &root, &fs).is_ok());
    }

    #[test]
    fn handle_marketplace_generate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_marketplace(&MarketplaceAction::Generate, &root, &fs).is_ok());
    }

    #[test]
    fn run_status_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        let cli = Cli {
            command: Commands::Status,
        };
        assert!(run(cli, &root, &fs).is_ok());
    }

    #[test]
    fn run_validate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        let cli = Cli {
            command: Commands::Validate,
        };
        assert!(run(cli, &root, &fs).is_ok());
    }

    #[test]
    fn validation_errors_propagate_nonzero_exit() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        let cli = Cli {
            command: Commands::Validate,
        };
        // `validate_all` flags the missing marketplace.json as an error; that must
        // surface as a non-zero exit (previously it printed but exited 0).
        assert_eq!(run(cli, &root, &fs).unwrap(), ExitCode::from(2));
    }

    #[test]
    fn report_and_exit_maps_errors_to_code_2() {
        use cmf::validation::{ValidationIssue, ValidationReport};
        assert_eq!(report_and_exit(&ValidationReport(vec![])), ExitCode::SUCCESS);
        assert_eq!(
            report_and_exit(&ValidationReport(vec![ValidationIssue::error("ctx", "boom")])),
            ExitCode::from(2)
        );
    }
}
