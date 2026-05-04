use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::json_file::load_json;

use crate::facet::{scan_facets, scan_recipes};
use crate::facet_types::{Facet, Recipe};
use crate::manifest::Platform;
use crate::plugin::{PluginInfo, scan_plugins};
use crate::plugin_types::Marketplace;
use crate::repo::{RepoKind, RepoRoot};
use crate::validate::validate_all;
use crate::validation::{IssueLevel, ValidationIssue};

pub fn format_plugin_list(plugins: &[PluginInfo]) -> String {
    let mut out = format!("Plugins ({}):\n", plugins.len());

    if plugins.is_empty() {
        return out;
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

        let _ = writeln!(
            out,
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
        );
    }

    out
}

pub fn format_facet_list(facets: &[Facet]) -> String {
    let mut out = format!("Facets ({}):\n", facets.len());

    if facets.is_empty() {
        return out;
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
        let _ = writeln!(out, "  {:<width$} {}", label, names.join(", "), width = max_cat_width);
    }

    out
}

pub fn format_recipe_list(recipes: &[Recipe]) -> String {
    let mut out = format!("Recipes ({}):\n", recipes.len());

    for recipe in recipes {
        let count = recipe.facets.len();
        let _ = writeln!(
            out,
            "  {} -> {} ({} {})",
            recipe.name,
            recipe.produces,
            count,
            if count == 1 { "facet" } else { "facets" }
        );
    }

    out
}

pub fn format_manifest_summary(files: &[PathBuf]) -> String {
    if files.is_empty() {
        return "No .claude-plugin/ sources found — nothing to generate.\n".to_string();
    }

    let mut out = format!("Generated manifests for {} platforms:\n", Platform::targets().len());

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

        let _ = writeln!(out, "  {dir_name}/ — {}", parts.join(" + "));
    }

    out
}

pub fn format_validation_issues(issues: &[ValidationIssue]) -> String {
    if issues.is_empty() {
        return "All plugins valid.\n".to_string();
    }

    let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
    let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

    let mut out = String::new();

    if !errors.is_empty() {
        out.push_str("Errors:\n");
        for issue in &errors {
            let _ = writeln!(out, "  {}: {}", issue.context, issue.message);
        }
    }

    if !warnings.is_empty() {
        out.push_str("Warnings:\n");
        for issue in &warnings {
            let _ = writeln!(out, "  {}: {}", issue.context, issue.message);
        }
    }

    out
}

pub fn format_status(root: &RepoRoot, fs: &dyn Filesystem) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format_repo_identity(root, fs));
    out.push_str(&format_plugin_summary(root, fs));
    out.push_str(&format_facet_summary(root, fs));
    out.push_str(&format_validation_summary(root, fs));
    Ok(out)
}

fn format_repo_identity(root: &RepoRoot, fs: &dyn Filesystem) -> String {
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
    let _ = writeln!(out, "Root: {}", root.path.display());
    out
}

fn format_plugin_summary(root: &RepoRoot, fs: &dyn Filesystem) -> String {
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

fn format_facet_summary(root: &RepoRoot, fs: &dyn Filesystem) -> String {
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
        let _ = writeln!(out, "Facets: {facet_count} ({breakdown})");
    }

    let Ok(recipes) = scan_recipes(root, fs) else {
        return out;
    };

    if !recipes.is_empty() {
        let recipe_count = recipes.len();
        let _ = writeln!(out, "Recipes: {recipe_count}");
    }

    out
}

fn format_validation_summary(root: &RepoRoot, fs: &dyn Filesystem) -> String {
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

pub fn format_recipe_assemble_result(produces: &str) -> String {
    format!("Wrote {produces}\n")
}

pub fn format_recipe_batch_result(count: usize) -> String {
    format!("Assembled {count} recipe(s)\n")
}

pub fn format_recipe_up_to_date(name: &str) -> String {
    format!("Recipe '{name}' is up to date\n")
}

pub fn format_recipe_diff(diff: &str) -> String {
    format!("{diff}\n")
}

pub fn format_plugin_create_result(name: &str, path: &Path) -> String {
    format!("Created plugin '{name}' at {}\n", path.display())
}

pub fn format_marketplace_generate_result(plugin_count: usize) -> String {
    format!("Generated marketplace.json with {plugin_count} plugins\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::plugin::PluginInfo;
    use crate::repo::RepoKind;
    use crate::test_support::{fake_marketplace_json_with_categories, fake_plugin_json};
    use cmx::gateway::fakes::FakeFilesystem;
    use cmx::types::{Artifact, ArtifactKind};

    fn fake_artifact(kind: ArtifactKind) -> Artifact {
        Artifact {
            kind,
            name: "test".to_string(),
            description: String::new(),
            path: PathBuf::from("/tmp/test"),
            version: None,
            deprecation: None,
        }
    }

    fn make_plugin(
        name: &str,
        version: Option<&str>,
        category: Option<&str>,
        agents: usize,
        skills: usize,
    ) -> PluginInfo {
        PluginInfo {
            name: name.to_string(),
            version: version.map(str::to_string),
            description: None,
            category: category.map(str::to_string),
            path: PathBuf::from("/tmp"),
            agents: (0..agents).map(|_| fake_artifact(ArtifactKind::Agent)).collect(),
            skills: (0..skills).map(|_| fake_artifact(ArtifactKind::Skill)).collect(),
        }
    }

    fn agent_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nAgent body.\n")
    }

    fn skill_md(description: &str) -> String {
        format!("---\ndescription: {description}\n---\n# Skill\n\nSkill body.\n")
    }

    fn facet_md(name: &str, category: &str, scope: &str) -> String {
        format!(
            "---\nname: {name}\nfacet: {category}\nscope: {scope}\n---\n# {name}\n\nFacet content.\n"
        )
    }

    fn recipe_json(name: &str, produces: &str, facets: &[&str]) -> String {
        let list: Vec<String> = facets.iter().map(|f| format!(r#""{f}""#)).collect();
        format!(
            r#"{{"name":"{name}","produces":"{produces}","facets":[{}],"runtime_skills":[]}}"#,
            list.join(",")
        )
    }

    #[test]
    fn format_plugin_list_empty() {
        let out = format_plugin_list(&[]);
        assert_eq!(out, "Plugins (0):\n");
    }

    #[test]
    fn format_plugin_list_single_plugin() {
        let plugins = vec![make_plugin("my-plugin", Some("1.0.0"), Some("tools"), 1, 2)];
        let out = format_plugin_list(&plugins);
        assert!(out.starts_with("Plugins (1):"));
        assert!(out.contains("my-plugin"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("tools"));
        assert!(out.contains("1 agent "));
        assert!(out.contains("2 skills"));
    }

    #[test]
    fn format_plugin_list_plural_agents() {
        let plugins = vec![make_plugin("my-plugin", None, None, 2, 1)];
        let out = format_plugin_list(&plugins);
        assert!(out.contains("2 agents"));
        assert!(out.contains("1 skill "));
    }

    #[test]
    fn format_plugin_list_missing_optional_fields() {
        let plugins = vec![make_plugin("my-plugin", None, None, 0, 0)];
        let out = format_plugin_list(&plugins);
        assert!(out.contains("my-plugin"));
        assert!(out.contains('-'));
    }

    #[test]
    fn status_marketplace_repo() {
        let fs = FakeFilesystem::new();

        let marketplace_json = fake_marketplace_json_with_categories(&[
            ("alpha", "Alpha tools", "./plugins/alpha", Some("ecosystem")),
            ("beta", "Beta tools", "./plugins/beta", Some("functional")),
        ]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", marketplace_json);
        fs.add_dir("/repo/plugins");

        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));
        fs.add_file("/repo/plugins/alpha/agents/reviewer.md", agent_md("reviewer", "Reviews code"));

        fs.add_file("/repo/plugins/beta/.claude-plugin/plugin.json", fake_plugin_json("beta"));
        fs.add_file("/repo/plugins/beta/skills/formatter/SKILL.md", skill_md("Formats code"));

        fs.add_dir("/repo/facets");
        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_md("engineering", "principles", "Core engineering"),
        );
        fs.add_file(
            "/repo/facets/principles/testing.md",
            facet_md("testing", "principles", "Testing practices"),
        );
        fs.add_dir("/repo/facets/recipes");
        fs.add_file(
            "/repo/facets/recipes/my-recipe.json",
            recipe_json(
                "my-recipe",
                "agents/my-agent.md",
                &["principles/engineering", "principles/testing"],
            ),
        );

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: true,
            has_plugins_dir: true,
        };

        let result = format_status(&root, &fs);
        assert!(result.is_ok(), "format_status failed: {:?}", result.err());
        let out = result.unwrap();
        assert!(out.contains("Repository:"));
        assert!(out.contains("Plugins:"));
    }

    #[test]
    fn status_empty_repo() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Unknown,
            has_facets: false,
            has_plugins_dir: false,
        };

        let result = format_status(&root, &fs);
        assert!(result.is_ok(), "format_status failed: {:?}", result.err());
    }
}
