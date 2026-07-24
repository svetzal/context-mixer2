//! Output formatting for facets, a submodule of `cmf/src/display/mod.rs`.

use std::fmt;

use cmx::table::render_table;

use crate::facet_types::{FacetList, RecipeList};

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

        let rows: Vec<Vec<String>> = groups
            .iter()
            .map(|(cat, names)| vec![format!("{cat}/"), names.join(", ")])
            .collect();
        write!(f, "{}", render_table(vec!["Category", "Facets"], 1, rows))?;

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::facet_types::{Facet, FacetList, Recipe, RecipeList};

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
}
