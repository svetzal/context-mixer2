# Working with Facets

Facets are authoring-time building blocks for composing agents. Each facet captures a single concern (error handling, testing strategy, code review practices) as a markdown file with structured frontmatter. Facets are not used at runtime -- they are assembled into agents via recipes.

## Directory structure

Facets live under `facets/` in your repository, organized by category:

```
facets/
├── principles/
│   ├── engineering.md
│   ├── code-review.md
│   └── testing.md
├── language/
│   ├── rust.md
│   └── python.md
├── testing/
│   └── tdd.md
└── recipes/
    └── rust-craftsperson.json
```

Each subdirectory under `facets/` is a category. The `recipes/` directory is special and contains recipe definitions (see [Recipes](./recipes.md)).

## Frontmatter format

Every facet file starts with YAML frontmatter:

```markdown
---
name: error-handling
facet: rust
scope: Error handling patterns and Result types
does-not-cover: Panic-based error handling
metadata:
  version: 1.0.0
---

# Error Handling

Your facet content here...
```

### Required fields

| Field | Description |
|-------|-------------|
| `name` | Kebab-case identifier, must match the filename (without `.md`) |
| `facet` | Category name, must match the parent directory name |

### Optional fields

| Field | Description |
|-------|-------------|
| `scope` | What this facet covers -- helps recipe authors choose facets |
| `does-not-cover` | Explicit boundaries -- what this facet intentionally excludes |
| `metadata.version` | Semver version for tracking changes |

The `version` field can also appear at the root level of frontmatter. If both are present, the root-level value takes precedence.

## Listing facets

```bash
cmf facet list
```

Displays all facets grouped by category, along with any recipes found.

## Validating facets

```bash
cmf facet validate
```

Checks:

- Required frontmatter fields (`name`, `facet`) are present and non-empty
- The `name` field matches the filename
- The `facet` field matches the parent directory
- No duplicate facet names within a category
- The `scope` field is present (warning if missing)
- Recipe facet references resolve to existing facets
