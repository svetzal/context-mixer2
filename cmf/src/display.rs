use std::collections::BTreeMap;
use std::fmt;

use cmx::gateway::Filesystem;
use cmx::json_file::load_json;
use cmx::platform::Platform;

use crate::facet::{scan_facets, scan_recipes};
use crate::facet_types::{FacetList, RecipeList};
use crate::manifest::ManifestSummary;
use crate::plugin::{PluginList, scan_plugins};
use crate::plugin_types::Marketplace;
use crate::repo::{RepoKind, RepoRoot};
use crate::validate::validate_all;
use crate::validation::{IssueLevel, ValidationReport};

impl fmt::Display for PluginList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plugins = &self.0;
        writeln!(f, "Plugins ({}):", plugins.len())?;

        if plugins.is_empty() {
            return Ok(());
        }

        let max_name = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0);
        let max_version = plugins
            .iter()
            .map(|p| p.version.as_deref().unwrap_or("-").len())
            .max()
            .unwrap_or(0);
        let max_category = plugins
            .iter()
            .map(|p| p.category.as_deref().unwrap_or("-").len())
            .max()
            .unwrap_or(0);

        for plugin in plugins {
            let version = plugin.version.as_deref().unwrap_or("-");
            let category = plugin.category.as_deref().unwrap_or("-");
            let agents = plugin.agents.len();
            let skills = plugin.skills.len();
            writeln!(
                f,
                "  {:<name_w$}  {:>ver_w$}  {:<cat_w$}  {} {}  {} {}",
                plugin.name,
                version,
                category,
                agents,
                if agents == 1 { "agent " } else { "agents" },
                skills,
                if skills == 1 { "skill " } else { "skills" },
                name_w = max_name,
                ver_w = max_version,
                cat_w = max_category,
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let issues = &self.0;
        if issues.is_empty() {
            return writeln!(f, "All plugins valid.");
        }

        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

        if !errors.is_empty() {
            writeln!(f, "Errors:")?;
            for issue in &errors {
                writeln!(f, "  {}: {}", issue.context, issue.message)?;
            }
        }

        if !warnings.is_empty() {
            writeln!(f, "Warnings:")?;
            for issue in &warnings {
                writeln!(f, "  {}: {}", issue.context, issue.message)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for ManifestSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let files = &self.0;
        if files.is_empty() {
            return writeln!(f, "No .claude-plugin/ sources found — nothing to generate.");
        }

        writeln!(f, "Generated manifests for {} platforms:", Platform::targets().len())?;

        for platform in Platform::targets() {
            let dir_name = platform.manifest_dir();

            let platform_files: Vec<_> = files
                .iter()
                .filter(|p| p.components().any(|c| c.as_os_str() == dir_name))
                .collect();

            let marketplace_count =
                platform_files.iter().filter(|p| p.ends_with("marketplace.json")).count();
            let plugin_count = platform_files.iter().filter(|p| p.ends_with("plugin.json")).count();

            let mut parts = Vec::new();
            if marketplace_count > 0 {
                parts.push("marketplace.json".to_string());
            }
            if plugin_count > 0 {
                parts.push(format!(
                    "{plugin_count} plugin manifest{}",
                    if plugin_count == 1 { "" } else { "s" }
                ));
            }

            writeln!(f, "  {dir_name}/ — {}", parts.join(" + "))?;
        }

        Ok(())
    }
}

impl fmt::Display for FacetList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let facets = &self.0;
        writeln!(f, "Facets ({}):", facets.len())?;

        if facets.is_empty() {
            return Ok(());
        }

        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for facet in facets {
            if let Some(last) = groups.last_mut() {
                if last.0 == facet.category {
                    last.1.push(facet.name.clone());
                    continue;
                }
            }
            groups.push((facet.category.clone(), vec![facet.name.clone()]));
        }

        let max_cat_width = groups.iter().map(|(cat, _)| cat.len() + 1).max().unwrap_or(0);

        for (category, names) in &groups {
            let label = format!("{category}/");
            writeln!(f, "  {:<width$} {}", label, names.join(", "), width = max_cat_width)?;
        }

        Ok(())
    }
}

impl fmt::Display for RecipeList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let recipes = &self.0;
        writeln!(f, "Recipes ({}):", recipes.len())?;

        for recipe in recipes {
            let count = recipe.facets.len();
            writeln!(
                f,
                "  {} -> {} ({} {})",
                recipe.name,
                recipe.produces,
                count,
                if count == 1 { "facet" } else { "facets" }
            )?;
        }

        Ok(())
    }
}

pub fn status_report(root: &RepoRoot, fs: &dyn Filesystem) -> String {
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
