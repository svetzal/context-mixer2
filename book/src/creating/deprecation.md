# Deprecation

Mark agents or skills as deprecated when they should no longer be used.

## Frontmatter fields

All three fields are progressive — use as many as apply:

```yaml
---
name: skill-writing
version: 1.0.0
deprecated: true
deprecated_reason: Superseded by Anthropic's skill-creator
deprecated_replacement: skill-creator
description: ...
---
```

| Field | Required | Description |
|-------|----------|-------------|
| `deprecated` | Yes | Set to `true` |
| `deprecated_reason` | No | Why it's deprecated — include actionable info |
| `deprecated_replacement` | No | Name of the replacement artifact |

## How it surfaces

### In `source browse`

```
Skills:
  blog-image-generator  v1.0.0
  skill-writing  v1.0.0  ⛔ DEPRECATED: Superseded by skill-creator (use skill-creator instead)
```

### In `list`

The status column shows ⛔ instead of ✅, even if the installed version matches the source.

### In `outdated`

Deprecated artifacts appear in the outdated list so users know to take action.

## Best practices

- Include actionable instructions in `deprecated_reason` — tell users how to get the replacement
- If the replacement is in a different source, include the `cmx source add` command in the reason
- Set `deprecated_replacement` only if the replacement is installable via cmx
