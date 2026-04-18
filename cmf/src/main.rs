use std::env;

use anyhow::Result;
use clap::Parser;
use cmx::gateway::{Filesystem, RealFilesystem};

use cmf::display::format_plugin_list;
use cmf::facet::{
    format_facet_list, format_recipe_list, scan_facets, scan_recipes, validate_facets,
};
use cmf::manifest::{format_manifest_summary, generate_manifests};
use cmf::marketplace::{generate_marketplace, validate_marketplace};
use cmf::plugin::{init_plugin, scan_plugins, validate_all_plugins};
use cmf::recipe::{assemble_recipe, diff_recipe, write_assembled};
use cmf::repo::{RepoRoot, detect_repo};
use cmf::status::format_status;
use cmf::validate::validate_all;
use cmf::validation::format_validation_issues;

mod cli;

use cli::{
    Cli, Commands, FacetAction, ManifestAction, MarketplaceAction, PluginAction, RecipeAction,
};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let fs = RealFilesystem;
    let cwd = env::current_dir()?;
    let root = detect_repo(&cwd, &fs)?;

    match cli.command {
        Commands::Facet { action } => handle_facet(&action, &root, &fs)?,
        Commands::Recipe { action } => handle_recipe(action, &root, &fs)?,
        Commands::Plugin { action } => handle_plugin(&action, &root, &fs)?,
        Commands::Manifest { action } => handle_manifest(&action, &root, &fs)?,
        Commands::Marketplace { action } => handle_marketplace(&action, &root, &fs)?,
        Commands::Validate => {
            let issues = validate_all(&root, &fs)?;
            print!("{}", format_validation_issues(&issues));
        }
        Commands::Status => {
            print!("{}", format_status(&root, &fs)?);
        }
    }

    Ok(())
}

fn handle_facet(action: &FacetAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        FacetAction::List => {
            let facets = scan_facets(root, fs)?;
            print!("{}", format_facet_list(&facets));
            let recipes = scan_recipes(root, fs)?;
            print!("{}", format_recipe_list(&recipes));
        }
        FacetAction::Validate => {
            let issues = validate_facets(root, fs)?;
            print!("{}", format_validation_issues(&issues));
        }
    }
    Ok(())
}

fn handle_recipe(action: RecipeAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        RecipeAction::List => {
            let recipes = scan_recipes(root, fs)?;
            print!("{}", format_recipe_list(&recipes));
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
    Ok(())
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

fn handle_plugin(action: &PluginAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        PluginAction::Init { name } => {
            let path = init_plugin(root, name, fs)?;
            println!("Created plugin '{}' at {}", name, path.display());
        }
        PluginAction::Validate => {
            let issues = validate_all_plugins(root, fs)?;
            print!("{}", format_validation_issues(&issues));
        }
        PluginAction::List => {
            let plugins = scan_plugins(root, fs)?;
            print!("{}", format_plugin_list(&plugins));
        }
    }
    Ok(())
}

fn handle_manifest(action: &ManifestAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        ManifestAction::Generate => {
            let written = generate_manifests(root, fs)?;
            print!("{}", format_manifest_summary(&written));
        }
    }
    Ok(())
}

fn handle_marketplace(
    action: &MarketplaceAction,
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Result<()> {
    match action {
        MarketplaceAction::Validate => {
            let issues = validate_marketplace(root, fs)?;
            print!("{}", format_validation_issues(&issues));
        }
        MarketplaceAction::Generate => {
            generate_marketplace(root, fs)?;
        }
    }
    Ok(())
}
