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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cmx::gateway::fakes::FakeFilesystem;

    use crate::facet_types::{Facet, FacetList, Recipe, RecipeList};
    use crate::manifest::ManifestSummary;
    use crate::plugin::{PluginInfo, PluginList};
    use crate::repo::{RepoKind, RepoRoot};
    use crate::test_support::{
        fake_marketplace_json, fake_marketplace_root_simple, fake_plugin_json,
    };
    use crate::validation::{IssueLevel, ValidationIssue, ValidationReport};

    use super::{
        facet_summary_str, plugin_summary_str, repo_identity_str, status_report,
        validation_summary_str,
    };

    fn unknown_root(path: &str) -> RepoRoot {
        RepoRoot {
            path: PathBuf::from(path),
            kind: RepoKind::Unknown,
            has_facets: false,
            has_plugins_dir: false,
        }
    }

    // --- PluginList Display ---

    #[test]
    fn plugin_list_display_empty() {
        assert!(PluginList(vec![]).to_string().starts_with("Plugins (0):"));
    }

    #[test]
    fn plugin_list_display_single_plugin() {
        let plugin = PluginInfo {
            name: "rust-craft".to_string(),
            version: Some("1.0.0".to_string()),
            description: None,
            category: Some("dev".to_string()),
            path: PathBuf::from("/plugins/rust-craft"),
            agents: vec![],
            skills: vec![],
        };
        let out = PluginList(vec![plugin]).to_string();
        assert!(out.contains("Plugins (1):"));
        assert!(out.contains("rust-craft"));
        assert!(out.contains("1.0.0"));
    }

    #[test]
    fn plugin_list_display_optional_fields_absent() {
        let plugin = PluginInfo {
            name: "bare-plugin".to_string(),
            version: None,
            description: None,
            category: None,
            path: PathBuf::from("/plugins/bare-plugin"),
            agents: vec![],
            skills: vec![],
        };
        let out = PluginList(vec![plugin]).to_string();
        assert!(out.contains("bare-plugin"));
        assert!(out.contains('-'));
    }

    // --- ManifestSummary Display ---

    #[test]
    fn manifest_summary_display_empty() {
        let out = ManifestSummary(vec![]).to_string();
        assert!(out.contains("nothing to generate"));
    }

    #[test]
    fn manifest_summary_display_with_files() {
        use cmx::platform::Platform;
        let dir = Platform::targets()[0].manifest_dir();
        let files = vec![
            PathBuf::from(format!("/{dir}/marketplace.json")),
            PathBuf::from(format!("/{dir}/plugin.json")),
        ];
        let out = ManifestSummary(files).to_string();
        assert!(out.contains("Generated manifests"));
        assert!(out.contains(dir));
    }

    // --- FacetList Display ---

    #[test]
    fn facet_list_display_empty() {
        assert_eq!(FacetList(vec![]).to_string(), "Facets (0):\n");
    }

    #[test]
    fn facet_list_display_single_category() {
        let facet = Facet {
            name: "error-handling".to_string(),
            category: "rust".to_string(),
            scope: None,
            does_not_cover: None,
            version: None,
            path: PathBuf::from("/facets/rust/error-handling.md"),
        };
        let out = FacetList(vec![facet]).to_string();
        assert!(out.contains("rust/"));
        assert!(out.contains("error-handling"));
    }

    #[test]
    fn facet_list_display_multiple_categories() {
        let f1 = Facet {
            name: "errors".to_string(),
            category: "rust".to_string(),
            scope: None,
            does_not_cover: None,
            version: None,
            path: PathBuf::from("/facets/rust/errors.md"),
        };
        let f2 = Facet {
            name: "testing".to_string(),
            category: "testing".to_string(),
            scope: None,
            does_not_cover: None,
            version: None,
            path: PathBuf::from("/facets/testing/testing.md"),
        };
        let out = FacetList(vec![f1, f2]).to_string();
        assert!(out.contains("rust/"));
        assert!(out.contains("testing/"));
    }

    // --- RecipeList Display ---

    #[test]
    fn recipe_list_display_empty() {
        assert_eq!(RecipeList(vec![]).to_string(), "Recipes (0):\n");
    }

    #[test]
    fn recipe_list_display_singular_facet() {
        let recipe = Recipe {
            name: "rust-agent".to_string(),
            description: String::new(),
            produces: "AGENTS.md".to_string(),
            facets: vec!["errors".to_string()],
            runtime_skills: vec![],
        };
        let out = RecipeList(vec![recipe]).to_string();
        assert!(out.contains("rust-agent"));
        assert!(out.contains("1 facet)"));
    }

    #[test]
    fn recipe_list_display_plural_facets() {
        let recipe = Recipe {
            name: "rust-agent".to_string(),
            description: String::new(),
            produces: "AGENTS.md".to_string(),
            facets: vec!["errors".to_string(), "testing".to_string()],
            runtime_skills: vec![],
        };
        let out = RecipeList(vec![recipe]).to_string();
        assert!(out.contains("2 facets)"));
    }

    // --- ValidationReport Display ---

    #[test]
    fn validation_report_display_clean() {
        let report = ValidationReport(vec![]);
        assert_eq!(report.to_string(), "All plugins valid.\n");
    }

    #[test]
    fn validation_report_display_with_errors_and_warnings() {
        let issues = vec![
            ValidationIssue {
                level: IssueLevel::Error,
                context: "p1".to_string(),
                message: "bad".to_string(),
            },
            ValidationIssue {
                level: IssueLevel::Warning,
                context: "p2".to_string(),
                message: "iffy".to_string(),
            },
        ];
        let out = ValidationReport(issues).to_string();
        assert!(out.contains("Errors:"));
        assert!(out.contains("bad"));
        assert!(out.contains("Warnings:"));
        assert!(out.contains("iffy"));
    }

    // --- repo_identity_str ---

    #[test]
    fn repo_identity_str_marketplace_with_name() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            r#"{"name":"My Marketplace","plugins":[]}"#,
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("My Marketplace"));
        assert!(out.contains("marketplace"));
    }

    #[test]
    fn repo_identity_str_plugin_without_name() {
        let fs = FakeFilesystem::new();
        let root = RepoRoot {
            path: PathBuf::from("/plugin"),
            kind: RepoKind::Plugin,
            has_facets: false,
            has_plugins_dir: false,
        };
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("(plugin)"));
    }

    #[test]
    fn repo_identity_str_unknown_kind() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/somewhere");
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("(unknown)"));
    }

    // --- plugin_summary_str ---

    #[test]
    fn plugin_summary_str_no_plugins_returns_empty() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"empty","plugins":[]}"#);
        let root = fake_marketplace_root_simple("/repo");
        assert_eq!(plugin_summary_str(&root, &fs), "");
    }

    #[test]
    fn plugin_summary_str_with_plugins_shows_counts() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("my-plugin", "A plugin", "./plugins/my-plugin")]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", json.as_str());
        fs.add_dir("/repo/plugins/my-plugin");
        fs.add_file(
            "/repo/plugins/my-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("my-plugin"),
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = plugin_summary_str(&root, &fs);
        assert!(out.contains("Plugins:"));
        assert!(out.contains('1'));
    }

    // --- facet_summary_str ---

    #[test]
    fn facet_summary_str_no_facets_flag_returns_empty() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/repo");
        assert_eq!(facet_summary_str(&root, &fs), "");
    }

    #[test]
    fn facet_summary_str_with_facets_shows_count() {
        use crate::test_support::fake_facet_content;
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/facets/rust/errors.md",
            fake_facet_content("errors", "rust", "Error handling"),
        );
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::FacetsOnly,
            has_facets: true,
            has_plugins_dir: false,
        };
        let out = facet_summary_str(&root, &fs);
        assert!(out.contains("Facets:"));
        assert!(out.contains('1'));
    }

    // --- validation_summary_str ---

    #[test]
    fn validation_summary_str_all_clean() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"clean","plugins":[]}"#);
        let root = fake_marketplace_root_simple("/repo");
        assert_eq!(validation_summary_str(&root, &fs), "Validation: all clean\n");
    }

    #[test]
    fn validation_summary_str_with_errors() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/nowhere");
        let out = validation_summary_str(&root, &fs);
        assert!(out.contains("Validation:"));
        assert!(out.contains("error"));
    }

    #[test]
    fn validation_summary_str_warnings_only() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("my-plugin", "A plugin", "./plugins/my-plugin")]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", json.as_str());
        fs.add_dir("/repo/plugins/my-plugin");
        fs.add_file(
            "/repo/plugins/my-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("mismatched-name"),
        );
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };
        let out = validation_summary_str(&root, &fs);
        assert!(out.contains("Validation:"));
        assert!(out.contains("warning"));
    }

    // --- status_report integration ---

    #[test]
    fn status_report_clean_marketplace() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            r#"{"name":"My Marketplace","plugins":[]}"#,
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = status_report(&root, &fs);
        assert!(out.contains("My Marketplace"));
        assert!(out.contains("Validation: all clean"));
    }

    #[test]
    fn status_report_unknown_repo_includes_identity() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/somewhere");
        let out = status_report(&root, &fs);
        assert!(out.contains("(unknown)"));
    }
}
