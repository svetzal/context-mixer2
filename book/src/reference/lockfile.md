# Lock File

cmx records installed state in JSON lock files.

## Locations

| Scope | Path | Purpose |
|-------|------|---------|
| Global | `~/.config/context-mixer/cmx-lock.json` | Personal install manifest |
| Local | `.context-mixer/cmx-lock.json` | Project-level, can be committed for team sharing |

## Format

```json
{
  "version": 1,
  "packages": {
    "python-craftsperson": {
      "type": "agent",
      "version": "1.3.1",
      "installed_at": "2026-03-20T10:30:00Z",
      "source": {
        "repo": "guidelines",
        "path": "agents/python-craftsperson.md"
      },
      "source_checksum": "sha256:abc123...",
      "installed_checksum": "sha256:abc123..."
    }
  }
}
```

## Fields

| Field | Description |
|-------|-------------|
| `type` | `agent` or `skill` |
| `version` | From frontmatter at install time (null if absent) |
| `installed_at` | ISO 8601 timestamp |
| `source.repo` | Name of the registered source |
| `source.path` | Relative path within the source |
| `source_checksum` | SHA-256 of the artifact in the source at install time |
| `installed_checksum` | SHA-256 of what was written to disk |

## How checksums are used

- **source_checksum vs current source** — detects upstream changes
- **installed_checksum vs current file** — detects local edits
- Both match at install time, then drift independently
