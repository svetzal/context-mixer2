# Creating Plugins

A plugin is a directory that bundles agents and skills into a distributable unit within a marketplace. Each plugin has its own metadata, version, and category.

## Plugin structure

```
my-plugin/
├── .claude-plugin/
│   └── plugin.json
├── agents/
│   ├── reviewer.md
│   └── planner.md
└── skills/
    └── code-review/
        └── SKILL.md
```

The `.claude-plugin/plugin.json` file identifies the directory as a plugin:

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "A collection of code review tools",
  "author": {
    "name": "Your Name",
    "email": "you@example.com"
  },
  "license": "MIT",
  "keywords": ["review", "quality"]
}
```

## Scaffolding a new plugin

From a marketplace repository root:

```bash
cmf plugin init my-new-plugin
```

This creates the full directory structure under `plugins/my-new-plugin/` with a starter `plugin.json` and empty `agents/` and `skills/` directories. You can then add agent `.md` files and skill directories.

Plugin init requires a marketplace repository (one with `.claude-plugin/marketplace.json`).

## Listing plugins

```bash
cmf plugin list
```

Reads the marketplace manifest and scans each plugin for agents and skills. Output includes name, version, category, and artifact counts.

## Validating plugins

```bash
cmf plugin validate
```

Checks each plugin listed in the marketplace:

- `plugin.json` exists and is valid JSON
- Plugin name is not empty
- Plugin name matches its directory name
- Agent `.md` files have valid frontmatter with a `name` field
- Skill directories contain a `SKILL.md` with a `description` field

## Plugin categories

Plugins can be organized into categories via the `category` field in `marketplace.json`:

| Category | Purpose |
|----------|---------|
| `ecosystem` | Language and framework tooling (e.g., rust-craft, python-craft) |
| `functional` | Cross-cutting capabilities (e.g., code-review, documentation) |
| `personal` | Individual workflow preferences and style guides |

Categories are optional. Plugins without a category appear as "uncategorized" in status output.
