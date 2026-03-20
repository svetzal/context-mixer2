# Writing Skills

A skill is a directory containing a `SKILL.md` file with YAML frontmatter, plus optional supporting files.

## Directory structure

```
my-skill/
├── SKILL.md              # Required: frontmatter + instructions
├── scripts/              # Optional: automation scripts
├── references/           # Optional: detailed documentation
└── examples/             # Optional: templates, samples
```

## Minimal skill

```markdown
---
name: my-skill
description: What this skill does and when to use it.
---

# My Skill

Instructions for the AI agent...
```

## Required fields

| Field | Description |
|-------|-------------|
| `name` | Kebab-case identifier — must match the parent directory name |
| `description` | What + when — used for matching against user requests |

## Optional cmx fields

| Field | Description |
|-------|-------------|
| `version` | Semver version |
| `deprecated` | `true` to mark as deprecated |
| `deprecated_reason` | Why it's deprecated |
| `deprecated_replacement` | Name of the replacement artifact |

## Follows the Agent Skills specification

cmx skills follow the [Agent Skills specification](https://agentskills.io/specification), making them compatible with Claude Code, GitHub Copilot, Cursor, and other AI coding assistants.

## Directory naming

The directory name becomes the artifact name. For example, `skills/blog-image-generator/SKILL.md` → artifact name `blog-image-generator`.
