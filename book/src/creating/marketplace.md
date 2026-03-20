# Marketplace Structure

A marketplace is a git repository that distributes agents and skills. cmx supports two formats.

## Plugin marketplace (recommended)

Add a `.claude-plugin/marketplace.json` to your repo. This is compatible with Claude Code's native plugin system.

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

### marketplace.json

```json
{
  "name": "my-marketplace",
  "owner": {
    "name": "Your Name",
    "email": "you@example.com"
  },
  "metadata": {
    "description": "My curated agents and skills",
    "version": "1.0.0"
  },
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

When `marketplace.json` is present, cmx reads it to discover artifacts rather than walking the directory tree.

## Simple directory (fallback)

Without `marketplace.json`, cmx walks the tree looking for:

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
