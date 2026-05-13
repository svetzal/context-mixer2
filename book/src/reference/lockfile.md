# Lock File

cmx records installed state in JSON lock files.

## Locations

Each platform has its own lock file so installations for different AI tools do not interfere.

| Scope | Claude Code | Other platforms |
|-------|-------------|-----------------|
| Global | `~/.config/context-mixer/cmx-lock.json` | `~/.config/context-mixer/cmx-lock-<platform>.json` |
| Local | `.context-mixer/cmx-lock.json` | `.context-mixer/cmx-lock-<platform>.json` |

Where `<platform>` is one of `copilot`, `cursor`, `windsurf`, or `gemini`.

Claude Code uses `cmx-lock.json` (no suffix) for backward compatibility.

Local lock files can be committed to version control for team sharing.

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
