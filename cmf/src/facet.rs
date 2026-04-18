use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::json_file::load_json;

use crate::facet_types::{Facet, Recipe, parse_facet};
use crate::repo::RepoRoot;
use crate::validation::ValidationIssue;

/// Scan all facets in the `facets/` directory (excluding `recipes/` and `README.md`).
///
/// Walks one level of subdirectories under `facets/`, treating each as a
/// category. Within each category, reads `.md` files (skipping `README.md`)
/// and parses their frontmatter.
pub fn scan_facets(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<Facet>> {
    if !root.has_facets {
        return Ok(Vec::new());
    }

    let facets_dir = root.path.join("facets");
    let entries = fs.read_dir(&facets_dir)?;
    let mut facets = Vec::new();

    for entry in entries {
        if !entry.is_dir || entry.file_name == "recipes" {
            continue;
        }

        let category_entries = fs.read_dir(&entry.path)?;
        for child in category_entries {
            if child.is_dir {
                continue;
            }
            let is_md = Path::new(&child.file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
            if !is_md || child.file_name.eq_ignore_ascii_case("README.md") {
                continue;
            }

            let content = fs.read_to_string(&child.path)?;
            if let Some(facet) = parse_facet(&child.path, &content) {
                facets.push(facet);
            }
        }
    }

    facets.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.name.cmp(&b.name)));
    Ok(facets)
}

/// Scan all recipes in `facets/recipes/`.
pub fn scan_recipes(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<Recipe>> {
    let recipes_dir = root.path.join("facets").join("recipes");
    if !fs.is_dir(&recipes_dir) {
        return Ok(Vec::new());
    }

    let entries = fs.read_dir(&recipes_dir)?;
    let mut recipes = Vec::new();

    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let is_json = Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if !is_json {
            continue;
        }

        let recipe: Recipe = load_json(&entry.path, fs)?;
        recipes.push(recipe);
    }

    recipes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(recipes)
}

/// Validate all facets and recipes, returning any issues found.
pub fn validate_facets(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let facets = scan_facets(root, fs)?;
    let recipes = scan_recipes(root, fs)?;
    let mut issues = Vec::new();

    validate_individual_facets(&facets, &mut issues);
    check_duplicate_facet_names(&facets, &mut issues);
    validate_individual_recipes(&recipes, &facets, &mut issues);

    Ok(issues)
}

fn validate_individual_facets(facets: &[Facet], issues: &mut Vec<ValidationIssue>) {
    for facet in facets {
        let context = format!("{}/{}", facet.category, facet.name);

        if facet.name.is_empty() {
            issues.push(ValidationIssue::error(context.clone(), "name field is empty"));
        }

        if facet.category.is_empty() {
            issues.push(ValidationIssue::error(context.clone(), "category (facet) field is empty"));
        }

        if facet.scope.is_none() {
            issues.push(ValidationIssue::warning(context.clone(), "scope field is empty"));
        }

        // Check that the facet name matches the filename (without .md)
        if let Some(stem) = facet.path.file_stem().and_then(|s| s.to_str()) {
            if stem != facet.name {
                issues.push(ValidationIssue::warning(
                    context.clone(),
                    format!("name \"{}\" does not match filename \"{}.md\"", facet.name, stem),
                ));
            }
        }

        // Check that the category matches the parent directory name
        if let Some(parent) =
            facet.path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str())
        {
            if parent != facet.category {
                issues.push(ValidationIssue::warning(
                    context,
                    format!(
                        "category \"{}\" does not match directory \"{}\"",
                        facet.category, parent
                    ),
                ));
            }
        }
    }
}

fn check_duplicate_facet_names(facets: &[Facet], issues: &mut Vec<ValidationIssue>) {
    let mut seen: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for facet in facets {
        let names = seen.entry(&facet.category).or_default();
        if !names.insert(&facet.name) {
            issues.push(ValidationIssue::warning(
                format!("{}/{}", facet.category, facet.name),
                format!(
                    "duplicate facet name \"{}\" in category \"{}\"",
                    facet.name, facet.category
                ),
            ));
        }
    }
}

fn validate_individual_recipes(
    recipes: &[Recipe],
    facets: &[Facet],
    issues: &mut Vec<ValidationIssue>,
) {
    for recipe in recipes {
        let context = format!("recipe:{}", recipe.name);

        if recipe.name.is_empty() {
            issues.push(ValidationIssue::error(context.clone(), "name field is empty"));
        }

        if recipe.produces.is_empty() {
            issues.push(ValidationIssue::error(context.clone(), "produces field is empty"));
        }

        let mut seen_refs = BTreeSet::new();
        for facet_ref in &recipe.facets {
            // Check for duplicates
            if !seen_refs.insert(facet_ref.as_str()) {
                issues.push(ValidationIssue::warning(
                    context.clone(),
                    format!("duplicate facet reference \"{facet_ref}\""),
                ));
                continue;
            }

            // Check that the reference resolves to an existing facet
            // Format: "category/name"
            let resolved = if let Some((cat, name)) = facet_ref.split_once('/') {
                facets.iter().any(|f| f.category == cat && f.name == name)
            } else {
                false
            };

            if !resolved {
                issues.push(ValidationIssue::error(
                    context.clone(),
                    format!("facet reference \"{facet_ref}\" does not resolve to any known facet"),
                ));
            }
        }
    }
}

/// Format a facet listing grouped by category.
pub fn format_facet_list(facets: &[Facet]) -> String {
    use std::fmt::Write as FmtWrite;

    let mut out = format!("Facets ({}):\n", facets.len());

    if facets.is_empty() {
        return out;
    }

    // Group facets by category, preserving sort order
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

/// Format a recipe listing.
pub fn format_recipe_list(recipes: &[Recipe]) -> String {
    use std::fmt::Write as FmtWrite;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoKind;
    use crate::test_support::{fake_facet_content, fake_recipe_json};
    use crate::validation::IssueLevel;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    fn facet_root(fs: &FakeFilesystem) -> RepoRoot {
        fs.add_dir("/repo/facets");
        RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::FacetsOnly,
            has_facets: true,
            has_plugins_dir: false,
        }
    }

    // -- scan_facets ----------------------------------------------------------

    #[test]
    fn scan_facets_finds_all() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("engineering", "principles", "Core engineering"),
        );
        fs.add_file(
            "/repo/facets/principles/code-review.md",
            fake_facet_content("code-review", "principles", "Code review practices"),
        );

        fs.add_dir("/repo/facets/language");
        fs.add_file(
            "/repo/facets/language/python.md",
            fake_facet_content("python", "language", "Python idioms"),
        );

        let facets = scan_facets(&root, &fs).unwrap();
        assert_eq!(facets.len(), 3);

        // Sorted by category then name
        assert_eq!(facets[0].category, "language");
        assert_eq!(facets[0].name, "python");
        assert_eq!(facets[1].category, "principles");
        assert_eq!(facets[1].name, "code-review");
        assert_eq!(facets[2].category, "principles");
        assert_eq!(facets[2].name, "engineering");
    }

    #[test]
    fn scan_facets_skips_recipes_and_readme() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        // A README.md in a category dir should be skipped
        fs.add_dir("/repo/facets/principles");
        fs.add_file("/repo/facets/principles/README.md", "# Principles\n\nOverview.\n");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("engineering", "principles", "Core engineering"),
        );

        // The recipes/ dir should be skipped entirely
        fs.add_dir("/repo/facets/recipes");
        fs.add_file(
            "/repo/facets/recipes/python-craftsperson.json",
            fake_recipe_json(
                "python-craftsperson",
                "agents/python-craftsperson.md",
                &["principles/engineering"],
            ),
        );

        // A top-level README.md in facets/ (not a directory, so read_dir of
        // facets/ returns it but it's not a dir and should be ignored)
        fs.add_file("/repo/facets/README.md", "# Facets\n\nTop-level readme.\n");

        let facets = scan_facets(&root, &fs).unwrap();
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0].name, "engineering");
    }

    #[test]
    fn scan_facets_empty() {
        let fs = FakeFilesystem::new();
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Unknown,
            has_facets: false,
            has_plugins_dir: false,
        };

        let facets = scan_facets(&root, &fs).unwrap();
        assert!(facets.is_empty());
    }

    // -- scan_recipes ---------------------------------------------------------

    #[test]
    fn scan_recipes_finds_all() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/recipes");
        fs.add_file(
            "/repo/facets/recipes/python-craftsperson.json",
            fake_recipe_json(
                "python-craftsperson",
                "agents/python-craftsperson.md",
                &["principles/engineering", "language/python"],
            ),
        );

        let recipes = scan_recipes(&root, &fs).unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].name, "python-craftsperson");
        assert_eq!(recipes[0].produces, "agents/python-craftsperson.md");
        assert_eq!(recipes[0].facets.len(), 2);
    }

    #[test]
    fn scan_recipes_empty() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);
        // No recipes/ dir exists

        let recipes = scan_recipes(&root, &fs).unwrap();
        assert!(recipes.is_empty());
    }

    // -- validate_facets ------------------------------------------------------

    #[test]
    fn validate_facets_ok() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("engineering", "principles", "Core engineering"),
        );

        fs.add_dir("/repo/facets/language");
        fs.add_file(
            "/repo/facets/language/python.md",
            fake_facet_content("python", "language", "Python idioms"),
        );

        fs.add_dir("/repo/facets/recipes");
        fs.add_file(
            "/repo/facets/recipes/my-recipe.json",
            fake_recipe_json(
                "my-recipe",
                "agents/my-agent.md",
                &["principles/engineering", "language/python"],
            ),
        );

        let issues = validate_facets(&root, &fs).unwrap();
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_facets_name_mismatch() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        // Filename is "engineering.md" but frontmatter name is "wrong-name"
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("wrong-name", "principles", "Core engineering"),
        );

        let issues = validate_facets(&root, &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("does not match filename"),
            "unexpected message: {}",
            warnings[0].message
        );
    }

    #[test]
    fn validate_facets_category_mismatch() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        // Directory is "principles" but frontmatter category is "wrong-category"
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("engineering", "wrong-category", "Core engineering"),
        );

        let issues = validate_facets(&root, &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert!(
            warnings.iter().any(|w| w.message.contains("does not match directory")),
            "expected category mismatch warning, got: {warnings:?}"
        );
    }

    #[test]
    fn validate_recipe_missing_facet() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            fake_facet_content("engineering", "principles", "Core engineering"),
        );

        fs.add_dir("/repo/facets/recipes");
        // Recipe references a facet that doesn't exist
        fs.add_file(
            "/repo/facets/recipes/bad-recipe.json",
            fake_recipe_json(
                "bad-recipe",
                "agents/bad.md",
                &["principles/engineering", "language/rust"],
            ),
        );

        let issues = validate_facets(&root, &fs).unwrap();
        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("does not resolve"),
            "unexpected message: {}",
            errors[0].message
        );
        assert!(errors[0].message.contains("language/rust"));
    }

    #[test]
    fn validate_facets_missing_scope() {
        let fs = FakeFilesystem::new();
        let root = facet_root(&fs);

        fs.add_dir("/repo/facets/principles");
        // Facet content without a scope field
        let content = "\
---
name: engineering
facet: principles
---
# Engineering

No scope provided.
";
        fs.add_file("/repo/facets/principles/engineering.md", content);

        let issues = validate_facets(&root, &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert!(
            warnings.iter().any(|w| w.message.contains("scope")),
            "expected scope warning, got: {warnings:?}"
        );
    }
}
