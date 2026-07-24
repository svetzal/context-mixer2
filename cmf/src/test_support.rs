//! Test helpers for generating fake marketplace/plugin JSON.

use std::path::PathBuf;

use cmx::gateway::fakes::FakeFilesystem;

use crate::repo::{RepoKind, RepoRoot};

/// Set up a fake marketplace repo and return its `RepoRoot`.
///
/// Writes `marketplace_json` to the `FakeFilesystem` under `/repo/.claude-plugin/marketplace.json`,
/// creates `/repo/plugins`, and returns a `RepoRoot` with `has_plugins_dir: true`.
pub fn fake_marketplace_root(fs: &FakeFilesystem, marketplace_json: &str) -> RepoRoot {
    fs.add_file("/repo/.claude-plugin/marketplace.json", marketplace_json);
    fs.add_dir("/repo/plugins");
    RepoRoot {
        path: PathBuf::from("/repo"),
        kind: RepoKind::Marketplace,
        has_facets: false,
        has_plugins_dir: true,
    }
}

/// Return a minimal marketplace `RepoRoot` for display tests that don't need filesystem state.
pub fn fake_marketplace_root_simple(path: &str) -> RepoRoot {
    RepoRoot {
        path: PathBuf::from(path),
        kind: RepoKind::Marketplace,
        has_facets: false,
        has_plugins_dir: false,
    }
}

/// Generate a valid `marketplace.json` string with the given plugin entries.
///
/// Each tuple is `(name, description, source_path)`.
pub fn fake_marketplace_json(plugins: &[(&str, &str, &str)]) -> String {
    let entries: Vec<String> = plugins
        .iter()
        .map(|(name, desc, source)| {
            format!(
                r#"    {{
      "name": "{name}",
      "description": "{desc}",
      "source": "{source}"
    }}"#
            )
        })
        .collect();

    format!(
        r#"{{
  "name": "svetzal-guidelines",
  "owner": {{
    "name": "Stacey Vetzal",
    "email": "stacey@vetzal.com"
  }},
  "plugins": [
{}
  ]
}}"#,
        entries.join(",\n")
    )
}

/// Generate a valid `marketplace.json` string with optional category per entry.
///
/// Each tuple is `(name, description, source_path, category)`.
pub fn fake_marketplace_json_with_categories(
    plugins: &[(&str, &str, &str, Option<&str>)],
) -> String {
    let entries: Vec<String> = plugins
        .iter()
        .map(|(name, desc, source, category)| {
            let cat_field = match category {
                Some(cat) => format!(",\n      \"category\": \"{cat}\""),
                None => String::new(),
            };
            format!(
                r#"    {{
      "name": "{name}",
      "description": "{desc}",
      "source": "{source}"{cat_field}
    }}"#
            )
        })
        .collect();

    format!(
        r#"{{
  "name": "svetzal-guidelines",
  "owner": {{
    "name": "Stacey Vetzal",
    "email": "stacey@vetzal.com"
  }},
  "plugins": [
{}
  ]
}}"#,
        entries.join(",\n")
    )
}

/// Generate a valid `plugin.json` string for the given plugin name.
pub fn fake_plugin_json(name: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "description": "A plugin named {name}",
  "author": {{
    "name": "Test Author",
    "email": "test@example.com"
  }}
}}"#
    )
}

/// Generate a valid facet markdown file with frontmatter.
pub fn fake_facet_content(name: &str, category: &str, scope: &str) -> String {
    format!(
        "\
---
name: {name}
facet: {category}
scope: {scope}
---
# {name}

Facet content for {name}.
"
    )
}

/// Generate a valid recipe JSON string.
pub fn fake_recipe_json(name: &str, produces: &str, facets: &[&str]) -> String {
    let facet_list: Vec<String> = facets.iter().map(|f| format!(r#""{f}""#)).collect();
    format!(
        r#"{{
  "name": "{name}",
  "description": "Recipe for {name}",
  "produces": "{produces}",
  "facets": [{}],
  "runtime_skills": []
}}"#,
        facet_list.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facet_types::{Recipe, parse_facet};
    use crate::plugin_types::{Marketplace, PluginManifest};
    use std::path::Path;

    #[test]
    fn fake_marketplace_json_is_valid() {
        let json = fake_marketplace_json(&[("test-plugin", "A test plugin", "./plugins/test")]);
        let mp: Marketplace = serde_json::from_str(&json).unwrap();
        assert_eq!(mp.name, "svetzal-guidelines");
        assert_eq!(mp.plugins.len(), 1);
        assert_eq!(mp.plugins[0].name, "test-plugin");
    }

    #[test]
    fn fake_plugin_json_is_valid() {
        let json = fake_plugin_json("my-plugin");
        let pm: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(pm.name, "my-plugin");
        assert_eq!(pm.version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn fake_facet_content_is_parseable() {
        let content = fake_facet_content("error-handling", "rust", "Error patterns");
        let facet = parse_facet(Path::new("/facets/error-handling.md"), &content).unwrap();
        assert_eq!(facet.name, "error-handling");
        assert_eq!(facet.category, "rust");
    }

    #[test]
    fn fake_recipe_json_is_valid() {
        let json = fake_recipe_json("rust-agent", "AGENTS.md", &["errors", "testing"]);
        let recipe: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(recipe.name, "rust-agent");
        assert_eq!(recipe.produces, "AGENTS.md");
        assert_eq!(recipe.facets, vec!["errors", "testing"]);
    }
}
