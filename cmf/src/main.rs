use std::collections::BTreeMap;
use std::env;

use anyhow::Result;
use clap::Parser;
use cmx::gateway::{Filesystem, RealFilesystem};
use cmx::json_file::load_json;

use cmf::facet::{scan_facets, scan_recipes, validate_facets};
use cmf::facet_types::{FacetList, RecipeList};
use cmf::manifest::{ManifestSummary, generate_manifests};
use cmf::marketplace::{generate_marketplace, validate_marketplace};
use cmf::plugin::{PluginList, init_plugin, scan_plugins, validate_all_plugins};
use cmf::plugin_types::Marketplace;
use cmf::recipe::{assemble_recipe, diff_recipe, write_assembled};
use cmf::repo::{RepoKind, RepoRoot, detect_repo};
use cmf::validate::validate_all;
use cmf::validation::{IssueLevel, ValidationReport};

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
            print!("{}", ValidationReport(issues));
        }
        Commands::Status => {
            print!("{}", status_report(&root, &fs));
        }
    }

    Ok(())
}

fn handle_facet(action: &FacetAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        FacetAction::List => {
            let facets = scan_facets(root, fs)?;
            print!("{}", FacetList(facets));
            let recipes = scan_recipes(root, fs)?;
            print!("{}", RecipeList(recipes));
        }
        FacetAction::Validate => {
            let issues = validate_facets(root, fs)?;
            print!("{}", ValidationReport(issues));
        }
    }
    Ok(())
}

fn handle_recipe(action: RecipeAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
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
            println!("Created plugin '{name}' at {}", path.display());
        }
        PluginAction::Validate => {
            let issues = validate_all_plugins(root, fs)?;
            print!("{}", ValidationReport(issues));
        }
        PluginAction::List => {
            let plugins = scan_plugins(root, fs)?;
            print!("{}", PluginList(plugins));
        }
    }
    Ok(())
}

fn handle_manifest(action: &ManifestAction, root: &RepoRoot, fs: &dyn Filesystem) -> Result<()> {
    match action {
        ManifestAction::Generate => {
            let written = generate_manifests(root, fs)?;
            print!("{}", ManifestSummary(written));
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
            print!("{}", ValidationReport(issues));
        }
        MarketplaceAction::Generate => {
            let count = generate_marketplace(root, fs)?;
            println!("Generated marketplace.json with {count} plugins");
        }
    }
    Ok(())
}

fn status_report(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let mut out = String::new();
    out.push_str(&repo_identity_str(root, fs));
    out.push_str(&plugin_summary_str(root, fs));
    out.push_str(&facet_summary_str(root, fs));
    out.push_str(&validation_summary_str(root, fs));
    out
}

fn repo_identity_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");
    let name = load_json::<Marketplace>(&marketplace_path, fs)
        .ok()
        .map(|m| m.name)
        .filter(|n| !n.is_empty());

    let kind_label = match root.kind {
        RepoKind::Marketplace => "marketplace",
        RepoKind::Plugin => "plugin",
        RepoKind::FacetsOnly => "facets-only",
        RepoKind::Unknown => "unknown",
    };

    let mut out = match name {
        Some(n) => format!("Repository: {n} ({kind_label})\n"),
        None => format!("Repository: ({kind_label})\n"),
    };
    let _ = std::fmt::write(&mut out, format_args!("Root: {}\n", root.path.display()));
    out
}

fn plugin_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let Ok(plugins) = scan_plugins(root, fs) else {
        return String::new();
    };

    if plugins.is_empty() {
        return String::new();
    }

    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_agents = 0usize;
    let mut total_skills = 0usize;

    for plugin in &plugins {
        let cat = plugin.category.as_deref().unwrap_or("uncategorized").to_string();
        *by_category.entry(cat).or_default() += 1;
        total_agents += plugin.agents.len();
        total_skills += plugin.skills.len();
    }

    let breakdown: Vec<String> =
        by_category.iter().map(|(cat, count)| format!("{count} {cat}")).collect();

    let plugin_count = plugins.len();
    let breakdown = breakdown.join(", ");
    format!(
        "Plugins: {plugin_count} ({breakdown})\nAgents: {total_agents} | Skills: {total_skills}\n"
    )
}

fn facet_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    if !root.has_facets {
        return String::new();
    }

    let Ok(facets) = scan_facets(root, fs) else {
        return String::new();
    };

    let mut out = String::new();

    if !facets.is_empty() {
        let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
        for facet in &facets {
            *by_category.entry(facet.category.clone()).or_default() += 1;
        }

        let breakdown: Vec<String> =
            by_category.iter().map(|(cat, count)| format!("{count} {cat}")).collect();

        let facet_count = facets.len();
        let breakdown = breakdown.join(", ");
        let _ = std::fmt::write(&mut out, format_args!("Facets: {facet_count} ({breakdown})\n"));
    }

    let Ok(recipes) = scan_recipes(root, fs) else {
        return out;
    };

    if !recipes.is_empty() {
        let recipe_count = recipes.len();
        let _ = std::fmt::write(&mut out, format_args!("Recipes: {recipe_count}\n"));
    }

    out
}

fn validation_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let Ok(issues) = validate_all(root, fs) else {
        return String::new();
    };

    if issues.is_empty() {
        return "Validation: all clean\n".to_string();
    }

    let errors = issues.iter().filter(|i| i.level == IssueLevel::Error).count();
    let warnings = issues.iter().filter(|i| i.level == IssueLevel::Warning).count();

    let mut parts = Vec::new();
    if errors > 0 {
        let label = if errors == 1 { "error" } else { "errors" };
        parts.push(format!("{errors} {label}"));
    }
    if warnings > 0 {
        let label = if warnings == 1 { "warning" } else { "warnings" };
        parts.push(format!("{warnings} {label}"));
    }

    let summary = parts.join(", ");
    format!("Validation: {summary}\n")
}
