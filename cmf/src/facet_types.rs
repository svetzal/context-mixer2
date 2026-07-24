//! Facet and Recipe structs, frontmatter parser.

use std::path::{Path, PathBuf};

use cmx::scan::{extract_field, extract_version, split_frontmatter_and_body};
use serde::{Deserialize, Serialize};

/// A single facet: a scoped, reusable piece of guidance parsed from a
/// markdown file's frontmatter and body.
#[derive(Debug, Clone)]
pub struct Facet {
    /// The facet's unique name, from the `name` frontmatter field.
    pub name: String,
    /// The category the facet belongs to, from the `facet` frontmatter field
    /// (e.g. `rust`, `git`).
    pub category: String,
    /// Optional free-text description of what the facet's guidance covers.
    pub scope: Option<String>,
    /// Optional free-text description of what the facet's guidance
    /// deliberately excludes.
    pub does_not_cover: Option<String>,
    /// Optional semver version, from either a top-level `version` field or a
    /// nested `metadata.version` field.
    pub version: Option<String>,
    /// Filesystem path the facet was parsed from.
    pub path: PathBuf,
}

/// A recipe: a named composition of facets assembled into a single output
/// agent document (e.g. `AGENTS.md`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Recipe {
    /// The recipe's unique name.
    pub name: String,
    /// Human-readable description of the assembled agent's purpose.
    #[serde(default)]
    pub description: String,
    /// Path (relative to the repository) the assembled output is written to.
    pub produces: String,
    /// Ordered list of facet names to compose, in the order they appear in
    /// the assembled output.
    pub facets: Vec<String>,
    /// Names of runtime skills to reference from the assembled agent.
    #[serde(default)]
    pub runtime_skills: Vec<String>,
}

/// Wrapper around a list of facets, used for grouped-by-category `Display`
/// output.
pub struct FacetList(pub Vec<Facet>);

/// Wrapper around a list of recipes, used for summary `Display` output.
pub struct RecipeList(pub Vec<Recipe>);

/// Parse facet frontmatter from a markdown file.
///
/// Returns `None` if the content doesn't have valid facet frontmatter
/// (missing `---` delimiters, or missing required `name` / `facet` fields).
pub fn parse_facet(path: &Path, content: &str) -> Option<Facet> {
    let (fm_opt, _) = split_frontmatter_and_body(content);
    let fm_text = fm_opt.as_deref()?;

    let name = extract_field(fm_text, "name")?;
    let category = extract_field(fm_text, "facet")?;
    let scope = extract_field(fm_text, "scope");
    let does_not_cover = extract_field(fm_text, "does-not-cover");
    let version = extract_version(fm_text);

    Some(Facet {
        name,
        category,
        scope,
        does_not_cover,
        version,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_facet(name: &str, category: &str) -> Facet {
        Facet {
            name: name.to_string(),
            category: category.to_string(),
            scope: None,
            does_not_cover: None,
            version: None,
            path: PathBuf::from(format!("/facets/{name}.md")),
        }
    }

    // --- Display for FacetList ---

    #[test]
    fn facet_list_display_empty() {
        assert_eq!(FacetList(vec![]).to_string(), "Facets (0):\n");
    }

    #[test]
    fn facet_list_display_groups_by_category() {
        let facets = vec![
            make_facet("error-handling", "rust"),
            make_facet("testing", "rust"),
            make_facet("commits", "git"),
        ];
        let out = FacetList(facets).to_string();
        assert!(out.starts_with("Facets (3):"));
        assert!(out.contains("rust/"));
        assert!(out.contains("git/"));
        assert!(out.contains("error-handling"));
        assert!(out.contains("commits"));
    }

    // --- Display for RecipeList ---

    #[test]
    fn recipe_list_display_empty() {
        assert_eq!(RecipeList(vec![]).to_string(), "Recipes (0):\n");
    }

    #[test]
    fn recipe_list_display_singular_facet() {
        let recipes = vec![Recipe {
            name: "my-recipe".to_string(),
            description: String::new(),
            produces: "AGENTS.md".to_string(),
            facets: vec!["rust/error-handling".to_string()],
            runtime_skills: Vec::new(),
        }];
        let out = RecipeList(recipes).to_string();
        assert!(out.contains("my-recipe"));
        assert!(out.contains("1 facet"));
        assert!(!out.contains("1 facets"));
    }

    #[test]
    fn recipe_list_display_plural_facets() {
        let recipes = vec![Recipe {
            name: "full-recipe".to_string(),
            description: String::new(),
            produces: "AGENTS.md".to_string(),
            facets: vec![
                "rust/error-handling".to_string(),
                "rust/testing".to_string(),
            ],
            runtime_skills: Vec::new(),
        }];
        let out = RecipeList(recipes).to_string();
        assert!(out.contains("2 facets"));
    }

    #[test]
    fn parse_valid_facet() {
        let content = "\
---
name: error-handling
facet: rust
scope: Error handling patterns and Result types
does-not-cover: Panic-based error handling
version: 1.0.0
---
# Error Handling

Content here.
";
        let facet = parse_facet(Path::new("/facets/error-handling.md"), content).unwrap();
        assert_eq!(facet.name, "error-handling");
        assert_eq!(facet.category, "rust");
        assert_eq!(facet.scope.as_deref(), Some("Error handling patterns and Result types"));
        assert_eq!(facet.does_not_cover.as_deref(), Some("Panic-based error handling"));
        assert_eq!(facet.version.as_deref(), Some("1.0.0"));
        assert_eq!(facet.path, PathBuf::from("/facets/error-handling.md"));
    }

    #[test]
    fn parse_facet_missing_name() {
        let content = "\
---
facet: rust
scope: Some scope
---
# Content
";
        assert!(parse_facet(Path::new("/facets/test.md"), content).is_none());
    }

    #[test]
    fn parse_facet_missing_facet_field() {
        let content = "\
---
name: my-facet
scope: Some scope
---
# Content
";
        assert!(parse_facet(Path::new("/facets/test.md"), content).is_none());
    }

    #[test]
    fn parse_facet_with_metadata_version() {
        let content = "\
---
name: testing
facet: rust
scope: Testing patterns
metadata:
  version: \"2.1.0\"
  author: Test
---
# Testing
";
        let facet = parse_facet(Path::new("/facets/testing.md"), content).unwrap();
        assert_eq!(facet.version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Just Markdown\n\nNo frontmatter here.\n";
        assert!(parse_facet(Path::new("/facets/plain.md"), content).is_none());
    }

    #[test]
    fn recipe_deserialize() {
        let json = r#"{
            "name": "rust-agent",
            "description": "A Rust development agent",
            "produces": "AGENTS.md",
            "facets": ["error-handling", "testing", "ownership"],
            "runtime_skills": ["clippy-fixer"]
        }"#;
        let recipe: Recipe = serde_json::from_str(json).unwrap();
        assert_eq!(recipe.name, "rust-agent");
        assert_eq!(recipe.produces, "AGENTS.md");
        assert_eq!(recipe.facets.len(), 3);
        assert_eq!(recipe.runtime_skills, vec!["clippy-fixer"]);

        // Round-trip
        let serialized = serde_json::to_string_pretty(&recipe).unwrap();
        let round_tripped: Recipe = serde_json::from_str(&serialized).unwrap();
        assert_eq!(recipe, round_tripped);
    }
}
