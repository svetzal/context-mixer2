use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cmx::gateway::{Filesystem, RealFilesystem};

use cmf::display::status_report;
use cmf::facet::{scan_facets, scan_recipes, validate_facets};
use cmf::facet_types::{FacetList, RecipeList};
use cmf::manifest::{ManifestSummary, generate_manifests};
use cmf::marketplace::{generate_marketplace, validate_marketplace};
use cmf::plugin::{PluginList, init_plugin, scan_plugins, validate_all_plugins};
use cmf::recipe::{assemble_recipe, diff_recipe, write_assembled};
use cmf::repo::{RepoRoot, detect_repo};
use cmf::validate::validate_all;
use cmf::validation::ValidationReport;

use cmf::cli::{
    Cli, Commands, FacetAction, ManifestAction, MarketplaceAction, PluginAction, RecipeAction,
};

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
        Commands::Facet { action } => handle_facet(&action, root, fs),
        Commands::Recipe { action } => handle_recipe(action, root, fs),
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

fn handle_facet(action: &FacetAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<ExitCode> {
    match action {
        FacetAction::List => {
            let facets = scan_facets(root, fs)?;
            print!("{}", FacetList(facets));
            let recipes = scan_recipes(root, fs)?;
            print!("{}", RecipeList(recipes));
            Ok(ExitCode::SUCCESS)
        }
        FacetAction::Validate => {
            let issues = validate_facets(root, fs)?;
            Ok(report_and_exit(&ValidationReport(issues)))
        }
    }
}

fn handle_recipe(action: RecipeAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<ExitCode> {
    match action {
        RecipeAction::List => {
            let recipes = scan_recipes(root, fs)?;
            print!("{}", RecipeList(recipes));
        }
        RecipeAction::Assemble { name, all } => {
            let recipes = scan_recipes(root, fs)?;
            if all {
                for recipe in &recipes {
                    let content = assemble_recipe(recipe, root, fs)?;
                    write_assembled(recipe, &content, root, fs)?;
                    println!("Wrote {}", recipe.produces);
                }
                println!("Assembled {} recipe(s)", recipes.len());
            } else if let Some(name) = name {
                let recipe = find_recipe(&recipes, &name)?;
                let content = assemble_recipe(recipe, root, fs)?;
                write_assembled(recipe, &content, root, fs)?;
                println!("Wrote {}", recipe.produces);
            } else {
                anyhow::bail!("Provide a recipe name or use --all");
            }
        }
        RecipeAction::Diff { name } => {
            let recipes = scan_recipes(root, fs)?;
            let recipe = find_recipe(&recipes, &name)?;
            let diff = diff_recipe(recipe, root, fs)?;
            if diff.is_empty() {
                println!("Recipe '{}' is up to date", recipe.name);
            } else {
                println!("{diff}");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn find_recipe<'a>(
    recipes: &'a [cmf::facet_types::Recipe],
    name: &str,
) -> Result<&'a cmf::facet_types::Recipe> {
    recipes
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Recipe '{name}' not found"))
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
            has_facets: false,
            has_plugins_dir: false,
        }
    }

    #[test]
    fn handle_facet_list_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_facet(&FacetAction::List, &root, &fs).is_ok());
    }

    #[test]
    fn handle_facet_validate_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_facet(&FacetAction::Validate, &root, &fs).is_ok());
    }

    #[test]
    fn handle_recipe_list_empty_returns_ok() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        assert!(handle_recipe(RecipeAction::List, &root, &fs).is_ok());
    }

    #[test]
    fn handle_recipe_assemble_no_name_no_all_errors() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        let result = handle_recipe(
            RecipeAction::Assemble {
                name: None,
                all: false,
            },
            &root,
            &fs,
        );
        assert!(result.is_err());
    }

    #[test]
    fn handle_recipe_diff_unknown_name_errors() {
        let root = unknown_root();
        let fs = FakeFilesystem::new();
        let result = handle_recipe(
            RecipeAction::Diff {
                name: "nonexistent".to_string(),
            },
            &root,
            &fs,
        );
        assert!(result.is_err());
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
