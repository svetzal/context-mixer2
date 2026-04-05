use std::fmt::Write as _;

use anyhow::Result;
use cmx::gateway::Filesystem;

use crate::facet_types::Recipe;
use crate::repo::RepoRoot;

/// Strip YAML frontmatter delimited by `---\n` from markdown content.
///
/// Returns everything after the closing `---\n`. If the content doesn't
/// start with `---\n` or has no second delimiter, the entire content is
/// returned unchanged.
fn strip_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---\n") else {
        return content;
    };
    match rest.find("---\n") {
        Some(end) => &rest[end + 4..],
        None => content,
    }
}

/// Assemble an agent `.md` file from a recipe by concatenating facet contents.
///
/// Returns the assembled content as a string including YAML frontmatter
/// derived from the recipe metadata.
pub fn assemble_recipe(recipe: &Recipe, root: &RepoRoot, fs: &dyn Filesystem) -> Result<String> {
    let mut output = String::new();

    // Agent frontmatter
    output.push_str("---\n");
    let _ = writeln!(output, "name: {}", recipe.name);
    let _ = writeln!(output, "description: {}", recipe.description);
    let _ = writeln!(output, "assembled_from: facets/recipes/{}.json", recipe.name);
    output.push_str("metadata:\n");
    output.push_str("  version: \"1.0.0\"\n");
    output.push_str("---\n");

    // Concatenate facet bodies
    for facet_ref in &recipe.facets {
        let facet_path = root.path.join("facets").join(format!("{facet_ref}.md"));
        let content = fs.read_to_string(&facet_path).map_err(|_| {
            anyhow::anyhow!(
                "Facet file not found for reference \"{facet_ref}\": {}",
                facet_path.display()
            )
        })?;
        let body = strip_frontmatter(&content);
        let trimmed = body.trim_start();
        output.push_str(trimmed);
        // Ensure each facet body ends with a newline
        if !trimmed.ends_with('\n') {
            output.push('\n');
        }
    }

    Ok(output)
}

/// Write the assembled agent to the `produces` path relative to the repo root.
pub fn write_assembled(
    recipe: &Recipe,
    content: &str,
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Result<()> {
    let target = root.path.join(&recipe.produces);
    // Ensure parent directory exists
    if let Some(parent) = target.parent() {
        fs.create_dir_all(parent)?;
    }
    fs.write(&target, content)?;
    Ok(())
}

/// Compare the assembled output against the current agent file on disk.
///
/// Returns a human-readable diff string. An empty string means the files
/// are identical. If the current file doesn't exist, the entire assembled
/// content is reported as a new file.
pub fn diff_recipe(recipe: &Recipe, root: &RepoRoot, fs: &dyn Filesystem) -> Result<String> {
    let assembled = assemble_recipe(recipe, root, fs)?;
    let target = root.path.join(&recipe.produces);

    if !fs.exists(&target) {
        return Ok(format!(
            "Recipe '{}' would create new file: {}\n({} lines)",
            recipe.name,
            recipe.produces,
            assembled.lines().count(),
        ));
    }

    let current = fs.read_to_string(&target)?;

    if current == assembled {
        return Ok(String::new());
    }

    let current_lines = current.lines().count();
    let assembled_lines = assembled.lines().count();

    Ok(format!(
        "Recipe '{}' differs from {}\n\
         --- current\n\
         +++ assembled\n\
         (line count: current={current_lines}, assembled={assembled_lines})",
        recipe.name, recipe.produces,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoKind;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    fn test_root(fs: &FakeFilesystem) -> RepoRoot {
        fs.add_dir("/repo/facets");
        RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::FacetsOnly,
            has_facets: true,
            has_plugins_dir: false,
        }
    }

    fn facet_with_frontmatter(name: &str, body: &str) -> String {
        format!("---\nname: {name}\nfacet: test\nscope: Test scope\n---\n{body}")
    }

    fn simple_recipe(name: &str, facets: &[&str]) -> Recipe {
        Recipe {
            name: name.to_string(),
            description: format!("Recipe for {name}"),
            produces: format!("agents/{name}.md"),
            facets: facets.iter().map(|s| (*s).to_string()).collect(),
            runtime_skills: Vec::new(),
        }
    }

    // -- strip_frontmatter ---------------------------------------------------

    #[test]
    fn strip_frontmatter_removes_yaml() {
        let content = "---\nname: test\nfacet: rust\n---\n# Heading\n\nBody text.\n";
        let result = strip_frontmatter(content);
        assert_eq!(result, "# Heading\n\nBody text.\n");
    }

    #[test]
    fn strip_frontmatter_no_frontmatter() {
        let content = "# Just Markdown\n\nNo frontmatter here.\n";
        let result = strip_frontmatter(content);
        assert_eq!(result, content);
    }

    // -- assemble_recipe -----------------------------------------------------

    #[test]
    fn assemble_single_facet() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);
        let body = "# Engineering\n\nBe excellent.\n";

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_with_frontmatter("engineering", body),
        );

        let recipe = simple_recipe("test-agent", &["principles/engineering"]);
        let result = assemble_recipe(&recipe, &root, &fs).unwrap();

        // Should have agent frontmatter
        assert!(result.starts_with("---\n"));
        assert!(result.contains("name: test-agent"));
        assert!(result.contains("assembled_from: facets/recipes/test-agent.json"));

        // Should have facet body without facet frontmatter
        assert!(result.contains("# Engineering"));
        assert!(result.contains("Be excellent."));
        assert!(!result.contains("facet: test"));
    }

    #[test]
    fn assemble_multi_facet() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_with_frontmatter("engineering", "# Engineering\n\nFirst.\n"),
        );
        fs.add_file(
            "/repo/facets/principles/code-review.md",
            facet_with_frontmatter("code-review", "# Code Review\n\nSecond.\n"),
        );

        fs.add_dir("/repo/facets/language");
        fs.add_file(
            "/repo/facets/language/python.md",
            facet_with_frontmatter("python", "# Python\n\nThird.\n"),
        );

        let recipe = simple_recipe(
            "multi",
            &[
                "principles/engineering",
                "principles/code-review",
                "language/python",
            ],
        );
        let result = assemble_recipe(&recipe, &root, &fs).unwrap();

        // All three bodies present in order
        let eng_pos = result.find("# Engineering").expect("engineering heading");
        let review_pos = result.find("# Code Review").expect("code-review heading");
        let python_pos = result.find("# Python").expect("python heading");
        assert!(eng_pos < review_pos);
        assert!(review_pos < python_pos);

        // No facet frontmatter leaked
        assert!(!result.contains("facet: test"));
        assert!(!result.contains("scope: Test scope"));
    }

    #[test]
    fn assemble_preserves_body_content() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        let body = "\
# Complex Content

Here is a code block:

```rust
fn main() {
    println!(\"hello\");
}
```

## Subheading

- Bullet one
- Bullet two

> Blockquote with `inline code`
";

        fs.add_dir("/repo/facets/testing");
        fs.add_file("/repo/facets/testing/complex.md", facet_with_frontmatter("complex", body));

        let recipe = simple_recipe("preserve", &["testing/complex"]);
        let result = assemble_recipe(&recipe, &root, &fs).unwrap();

        assert!(result.contains("```rust"));
        assert!(result.contains("fn main()"));
        assert!(result.contains("## Subheading"));
        assert!(result.contains("- Bullet one"));
        assert!(result.contains("> Blockquote with `inline code`"));
    }

    #[test]
    fn assemble_missing_facet() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        let recipe = simple_recipe("bad", &["nonexistent/missing"]);
        let result = assemble_recipe(&recipe, &root, &fs);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent/missing"),
            "error should mention the missing facet ref: {err}"
        );
    }

    // -- diff_recipe ---------------------------------------------------------

    #[test]
    fn diff_identical() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_with_frontmatter("engineering", "# Engineering\n\nContent.\n"),
        );

        let recipe = simple_recipe("same", &["principles/engineering"]);

        // First assemble to get the expected content
        let assembled = assemble_recipe(&recipe, &root, &fs).unwrap();

        // Write that content to the produces path
        fs.add_dir("/repo/agents");
        fs.add_file("/repo/agents/same.md", assembled);

        let diff = diff_recipe(&recipe, &root, &fs).unwrap();
        assert!(diff.is_empty(), "expected empty diff, got: {diff}");
    }

    #[test]
    fn diff_different() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_with_frontmatter("engineering", "# Engineering\n\nContent.\n"),
        );

        let recipe = simple_recipe("changed", &["principles/engineering"]);

        // Write different content to the produces path
        fs.add_dir("/repo/agents");
        fs.add_file("/repo/agents/changed.md", "Old stale content.\n");

        let diff = diff_recipe(&recipe, &root, &fs).unwrap();
        assert!(!diff.is_empty(), "expected non-empty diff");
        assert!(diff.contains("changed"));
        assert!(diff.contains("--- current"));
        assert!(diff.contains("+++ assembled"));
    }

    #[test]
    fn diff_new_file() {
        let fs = FakeFilesystem::new();
        let root = test_root(&fs);

        fs.add_dir("/repo/facets/principles");
        fs.add_file(
            "/repo/facets/principles/engineering.md",
            facet_with_frontmatter("engineering", "# Engineering\n\nContent.\n"),
        );

        let recipe = simple_recipe("brand-new", &["principles/engineering"]);

        // Don't create the produces file — it's new
        let diff = diff_recipe(&recipe, &root, &fs).unwrap();
        assert!(!diff.is_empty(), "expected non-empty diff for new file");
        assert!(diff.contains("new file"));
        assert!(diff.contains("brand-new"));
    }
}
