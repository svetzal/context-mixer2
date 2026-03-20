# Writing Agents

An agent is a single `.md` file with YAML frontmatter that provides curated guidance for a tech stack.

## Minimal agent

```markdown
---
name: python-craftsperson
description: |
  Use this agent when writing, reviewing, or maintaining Python code.
model: sonnet
---

You are an expert Python developer...
```

## Required fields

| Field | Description |
|-------|-------------|
| `name` | Kebab-case identifier |
| `description` | When to use this agent — include examples for better matching |
| `model` | `sonnet`, `opus`, `haiku`, or `inherit` |

## Optional cmx fields

| Field | Description |
|-------|-------------|
| `version` | Semver version (e.g., `1.3.1`) |
| `deprecated` | `true` to mark as deprecated |
| `deprecated_reason` | Why it's deprecated |
| `deprecated_replacement` | Name of the replacement artifact |

## Agent body

The markdown body after the frontmatter closing `---` becomes the agent's system prompt. Write it in second person ("You are...").

## File naming

The file name (without `.md`) becomes the artifact name used by cmx. For example, `python-craftsperson.md` → artifact name `python-craftsperson`.
