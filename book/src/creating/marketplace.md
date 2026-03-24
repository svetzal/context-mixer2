# Marketplace Structure

A marketplace is a git repository that distributes agents and skills. cmx supports the Claude Code plugin marketplace format and a simple fallback.

## Plugin marketplace (recommended)

Add a `.claude-plugin/marketplace.json` to your repo. This is compatible with Claude Code's native plugin system.

cmx supports three plugin declaration styles within `marketplace.json`, matching the formats used across Anthropic's own repositories.

### Format 1: Explicit `agents`/`skills` arrays

Best when you want precise control over which artifacts are exposed:

```
my-marketplace/
├── .claude-plugin/
│   └── marketplace.json
├── agents/
│   ├── python-craftsperson.md
│   └── typescript-craftsperson.md
├── skills/
│   ├── blog-image-generator/
│   │   └── SKILL.md
│   └── code-review/
│       └── SKILL.md
└── README.md
```

```json
{
  "name": "my-marketplace",
  "owner": { "name": "Your Name" },
  "plugins": [
    {
      "name": "my-plugin",
      "description": "Description of this collection",
      "source": "./",
      "agents": [
        "./agents/python-craftsperson.md",
        "./agents/typescript-craftsperson.md"
      ],
      "skills": [
        "./skills/blog-image-generator",
        "./skills/code-review"
      ]
    }
  ]
}
```

Paths in `agents` and `skills` arrays are resolved relative to the repository root.

### Format 2: Source path without explicit arrays

Best when each plugin lives in its own subdirectory. cmx walks the directory to discover artifacts automatically:

```
my-marketplace/
├── .claude-plugin/
│   └── marketplace.json
├── plugins/
│   ├── code-review/
│   │   ├── reviewer.md
│   │   └── review-skill/
│   │       └── SKILL.md
│   └── commit-tools/
│       └── committer.md
└── README.md
```

```json
{
  "name": "my-marketplace",
  "owner": { "name": "Your Name" },
  "plugins": [
    {
      "name": "code-review",
      "description": "Automated code review",
      "source": "./plugins/code-review"
    },
    {
      "name": "commit-tools",
      "description": "Git commit workflows",
      "source": "./plugins/commit-tools"
    }
  ]
}
```

When no `agents`/`skills` arrays are present, cmx resolves the `source` path and walks that directory to find `.md` agents and `SKILL.md` skills.

### Format 3: Remote source objects (not yet supported)

The official Claude Code plugin format also supports remote sources (`url`, `github`, `git-subdir`, `npm`). cmx recognizes these entries and emits a warning. Future versions may add support for fetching remote plugin sources.

```json
{
  "plugins": [
    {
      "name": "some-plugin",
      "source": {
        "source": "url",
        "url": "https://github.com/example/plugin.git",
        "sha": "a1b2c3d4..."
      }
    }
  ]
}
```

### Scanning priority

1. If `agents`/`skills` arrays exist on a plugin entry, cmx uses them directly.
2. If only a `source` path string is present, cmx walks that directory.
3. If `source` is an object (remote), cmx warns and skips.

## Simple directory (fallback)

Without `marketplace.json`, cmx walks the entire tree looking for:

- `.md` files with frontmatter containing `name` and `description` → agents
- Directories containing `SKILL.md` with frontmatter → skills

This works for quick setups but the marketplace format is preferred for explicit control and Claude Code compatibility.

## Registering with cmx

```bash
# As a git source
cmx source add my-marketplace https://github.com/you/my-marketplace

# As a local directory
cmx source add my-marketplace ~/Work/my-marketplace
```

## Compatibility

A marketplace registered with cmx can also be used directly with Claude Code:

```
/plugin marketplace add https://github.com/you/my-marketplace
```
