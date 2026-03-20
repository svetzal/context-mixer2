# Versioning

cmx tracks artifact identity with two complementary mechanisms.

## Checksums (automatic)

SHA-256 checksums are always computed — no author effort required. They detect *that* something changed, but not *what* or *how significant*.

| Comparison | Detects |
|-----------|---------|
| Installed vs source | Source has been updated since install |
| Installed vs lock file | Local copy was hand-edited |

## Versions (opt-in)

Add a `version` field to your frontmatter:

```yaml
---
name: python-craftsperson
description: ...
version: 1.3.1
model: sonnet
---
```

Versions communicate significance of changes. Use [semantic versioning](https://semver.org/):

- **Major** (2.0.0) — significant rewrites, changed philosophy or approach
- **Minor** (1.1.0) — new capabilities, added guidance
- **Patch** (1.0.1) — small fixes, formatting, metadata updates

## How they work together

| Installed | Source | cmx shows |
|-----------|--------|-----------|
| Checksums match | — | ✅ Up to date |
| Versions differ | 1.0 → 1.1 | ⚠️ Update available |
| No version, checksum differs | — | ⚠️ Source has changed |
| No lock entry, source has version | — | ⚠️ Untracked |

**Design principle**: checksums are free and always present; versions are opt-in and add human-meaningful context when authors choose to provide them.
