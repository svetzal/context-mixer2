use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;

use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::json_file::load_json;

use crate::facet::{scan_facets, scan_recipes};
use crate::plugin::scan_plugins;
use crate::plugin_types::Marketplace;
use crate::repo::{RepoKind, RepoRoot};
use crate::validate::validate_all;
use crate::validation::IssueLevel;

/// Format a comprehensive status overview of the repo.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoKind;
    use crate::test_support::{fake_marketplace_json_with_categories, fake_plugin_json};
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

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
    fn status_marketplace_repo() {
        let fs = FakeFilesystem::new();

        let marketplace_json = fake_marketplace_json_with_categories(&[
            ("alpha", "Alpha tools", "./plugins/alpha", Some("ecosystem")),
            ("beta", "Beta tools", "./plugins/beta", Some("functional")),
        ]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", marketplace_json);
        fs.add_dir("/repo/plugins");

        // alpha has 1 agent
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));
        fs.add_file("/repo/plugins/alpha/agents/reviewer.md", agent_md("reviewer", "Reviews code"));

        // beta has 1 skill
        fs.add_file("/repo/plugins/beta/.claude-plugin/plugin.json", fake_plugin_json("beta"));
        fs.add_file("/repo/plugins/beta/skills/formatter/SKILL.md", skill_md("Formats code"));

        // Add facets
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
