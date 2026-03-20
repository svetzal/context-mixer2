# Frontmatter Reference

## Agent frontmatter

```yaml
---
name: python-craftsperson
description: |
  Use this agent when writing, reviewing, or maintaining Python code.
version: 1.3.1
model: sonnet
deprecated: true
deprecated_reason: Replaced by python-craftsperson-v2
deprecated_replacement: python-craftsperson-v2
---
```

| Field | Required | Type | Description |
|-------|----------|------|-------------|
| `name` | Yes | string | Kebab-case identifier |
| `description` | Yes | string | When to use — include examples for matching |
| `model` | Yes | string | `sonnet`, `opus`, `haiku`, or `inherit` |
| `version` | No | string | Semver version |
| `deprecated` | No | boolean | `true` to mark deprecated |
| `deprecated_reason` | No | string | Why deprecated — include actionable info |
| `deprecated_replacement` | No | string | Replacement artifact name |

## Skill frontmatter (SKILL.md)

```yaml
---
name: blog-image-generator
description: Generate images for blog posts.
version: 1.0.0
---
```

| Field | Required | Type | Description |
|-------|----------|------|-------------|
| `name` | Yes | string | Kebab-case, must match parent directory name |
| `description` | Yes | string | What + when to use |
| `version` | No | string | Semver version |
| `deprecated` | No | boolean | `true` to mark deprecated |
| `deprecated_reason` | No | string | Why deprecated |
| `deprecated_replacement` | No | string | Replacement artifact name |

## marketplace.json

```json
{
  "name": "marketplace-id",
  "owner": { "name": "Author", "email": "author@example.com" },
  "metadata": { "description": "...", "version": "1.0.0" },
  "plugins": [{
    "name": "plugin-id",
    "description": "...",
    "source": "./",
    "agents": ["./agents/my-agent.md"],
    "skills": ["./skills/my-skill"]
  }]
}
```
