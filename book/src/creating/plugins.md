# Creating Plugins

A plugin is a directory that bundles agents and skills into a distributable unit within a marketplace. Each plugin has its own metadata, version, and category.

## Plugin structure

```text
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

Plugin scaffolding, validation, marketplace generation, and manifest
projection are outside cmf. cmf materializes structured intents and installs
the resulting artifact directly through cmx-core; use the repository's chosen
publishing workflow when a distributable plugin is actually required.

## Plugin categories

Plugins can be organized into categories via the `category` field in `marketplace.json`:

| Category | Purpose |
|----------|---------|
| `ecosystem` | Language and framework tooling (e.g., rust-craft, python-craft) |
| `functional` | Cross-cutting capabilities (e.g., code-review, documentation) |
| `personal` | Individual workflow preferences and style guides |

Categories are optional. Plugins without a category appear as "uncategorized" in status output.
