# Recipe Assembly

Recipes declare how to assemble an agent from facets. A recipe is a JSON file in `facets/recipes/` that lists which facets to combine and where to write the result.

## Recipe format

```json
{
  "name": "rust-craftsperson",
  "description": "A Rust development agent assembled from principles and language facets",
  "produces": "agents/rust-craftsperson.md",
  "facets": [
    "principles/engineering",
    "principles/testing",
    "language/rust"
  ],
  "runtime_skills": [
    "clippy-fixer"
  ]
}
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Recipe identifier |
| `description` | No | Human-readable purpose |
| `produces` | Yes | Output file path relative to the repo root |
| `facets` | Yes | Ordered list of facet references (`category/name`) |
| `runtime_skills` | No | Skills the assembled agent should have available at runtime |

Facet references use the format `category/name`, matching the directory structure under `facets/`.

## Listing recipes

```bash
cmf recipe list
```

Shows each recipe with its output path and facet count.

## Assembling agents

Assemble a single recipe:

```bash
cmf recipe assemble rust-craftsperson
```

Assemble all recipes at once:

```bash
cmf recipe assemble --all
```

Assembly works by:

1. Resolving each facet reference to its markdown file
2. Stripping the YAML frontmatter from each facet
3. Concatenating the remaining content in order
4. Writing the result to the `produces` path

This is a naive concatenation -- no deduplication or conflict resolution. The order of facets in the recipe determines the order in the output.

## Diffing

Check whether an assembled output would differ from the current file on disk:

```bash
cmf recipe diff rust-craftsperson
```

If the recipe is up to date, it reports so. Otherwise it shows a unified diff of the changes that `assemble` would make. This is useful for verifying that manual edits to an agent file haven't drifted from the recipe definition.
