use std::path::{Path, PathBuf};

use cmx::scan::{extract_field, extract_version};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Facet {
    pub name: String,
    pub category: String,
    pub scope: Option<String>,
    pub does_not_cover: Option<String>,
    pub version: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Recipe {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub produces: String,
    pub facets: Vec<String>,
    #[serde(default)]
    pub runtime_skills: Vec<String>,
}

/// Parse facet frontmatter from a markdown file.
///
/// Returns `None` if the content doesn't have valid facet frontmatter
/// (missing `---` delimiters, or missing required `name` / `facet` fields).
pub fn parse_facet(path: &Path, content: &str) -> Option<Facet> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let fm_text = &rest[..end];

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
