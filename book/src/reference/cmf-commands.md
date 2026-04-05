# cmf Command Reference

cmf is the publisher and authoring tool for context mixer artifacts. Run all commands from the root of a marketplace, plugin, or facets repository.

## Status

| Command | Description |
|---------|-------------|
| `cmf status` | Show repository overview: kind, plugins, facets, validation summary |

Example output:

```
Repository: svetzal-guidelines (marketplace)
Root: /home/user/guidelines
Plugins: 13 (11 ecosystem, 1 functional, 1 personal)
Agents: 16 | Skills: 7
Facets: 7 (5 principles, 1 language, 1 testing)
Recipes: 1
Validation: all clean
```

## Validation

| Command | Description |
|---------|-------------|
| `cmf validate` | Run all validation checks (marketplace, plugins, facets, recipes) |

Prints errors first, then warnings. Exit code is 0 even with issues (use the output to determine status).

## Plugin management

| Command | Description |
|---------|-------------|
| `cmf plugin list` | List all plugins with version, category, and artifact counts |
| `cmf plugin init <name>` | Scaffold a new plugin directory under `plugins/` |
| `cmf plugin validate` | Validate all plugin structures and frontmatter |

`plugin init` requires a marketplace repository. It creates `plugins/<name>/` with `.claude-plugin/plugin.json`, `agents/`, and `skills/` directories.

## Marketplace

| Command | Description |
|---------|-------------|
| `cmf marketplace validate` | Check marketplace.json against actual plugin directories |
| `cmf marketplace generate` | Generate or update marketplace.json from the plugins directory |

`marketplace generate` discovers plugins under `plugins/`, reads each `plugin.json`, and writes `marketplace.json`. Existing entries are preserved (including categories and metadata); new plugins are appended.

## Facet management

| Command | Description |
|---------|-------------|
| `cmf facet list` | List all facets grouped by category, plus recipes |
| `cmf facet validate` | Validate facet frontmatter, naming, and recipe references |

## Recipe management

| Command | Description |
|---------|-------------|
| `cmf recipe list` | List all recipes with output paths and facet counts |
| `cmf recipe assemble <name>` | Assemble an agent from a recipe's facets |
| `cmf recipe assemble --all` | Assemble all recipes |
| `cmf recipe diff <name>` | Show diff between assembled output and current file |

## Manifest generation

| Command | Description |
|---------|-------------|
| `cmf manifest generate` | Generate multi-platform manifests from `.claude-plugin/` sources |

Reads plugin metadata from `.claude-plugin/plugin.json` or `marketplace.json` and writes equivalent manifests for other platforms:

- `.codex-plugin/plugin.json`
- `.cursor-plugin/plugin.json`
- `gemini-extension.json`
