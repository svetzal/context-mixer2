# cmx — Package Manager for Curated Agentic Context

> Working draft. Expect iteration.

## What cmx does

cmx manages the lifecycle of **agents** and **skills** — versioning, installation, updates, and distribution across AI coding assistants. It works with **plugin marketplaces** — git repositories that follow the Claude Code plugin specification.

**Key distinction from hone**: hone derives ad-hoc agents from a repo's existing structure. cmx curates, distributes, and manages polished agents and composable skills from marketplace repositories.

## Artifact types

| Artifact | Shape | Purpose |
|----------|-------|---------|
| **Agent** | Single `.md` file with YAML frontmatter | Portable, curated guidance for a tech stack — applies across many repositories |
| **Skill** | Directory with `SKILL.md` + supporting files | Composable tool capability — task-specific functionality the agent can invoke |

Agents and skills can be installed globally (`~/.claude/agents/`, `~/.claude/skills/`) or locally in a project (`.claude/agents/`, `.claude/skills/`).

### Agent frontmatter

```yaml
---
name: python-craftsperson
description: |
  Use this agent when writing, reviewing, or maintaining Python code.
version: 1.3.1
model: sonnet
---
```

Optional cmx fields (backward compatible with Claude Code): `version`, `deprecated`, `deprecated_reason`, `deprecated_replacement`.

### Skill directory structure

Follows the [Agent Skills specification](https://agentskills.io/specification):

```
my-skill/
├── SKILL.md              # Required: frontmatter + instructions
├── scripts/              # Optional: deterministic automation
├── references/           # Optional: detailed documentation
└── examples/             # Optional: templates, samples
```

### Deprecation

Any agent or skill can be marked deprecated via frontmatter:

```yaml
deprecated: true
deprecated_reason: Superseded by X with better Y support
deprecated_replacement: replacement-name
```

All three fields are progressive — `deprecated: true` alone is valid. cmx surfaces deprecation in `browse`, `list`, and `outdated` views.

## Source repositories (marketplaces)

The primary distribution mechanism is the **plugin marketplace** — a git repository following the Claude Code plugin specification. A marketplace contains a `.claude-plugin/marketplace.json` manifest that catalogs its plugins, each of which can contain agents and skills.

### Marketplace structure

```
my-marketplace/
├── .claude-plugin/
│   └── marketplace.json      # Required: marketplace manifest
├── agents/                    # Agent .md files
│   ├── python-craftsperson.md
│   └── typescript-craftsperson.md
├── skills/                    # Skill directories
│   ├── blog-image-generator/
│   │   └── SKILL.md
│   └── skill-writing/
│       └── SKILL.md
└── README.md
```

### marketplace.json

cmx supports multiple plugin declaration styles within `marketplace.json`, matching the formats used by Anthropic's own repositories.

#### Format 1: Explicit `agents`/`skills` arrays

Used when the marketplace author wants precise control over which artifacts are exposed (e.g. `anthropics/skills`):

```json
{
  "name": "my-marketplace",
  "owner": { "name": "Author Name" },
  "plugins": [
    {
      "name": "craftsperson-agents",
      "source": "./",
      "agents": [
        "./agents/python-craftsperson.md",
        "./agents/typescript-craftsperson.md"
      ],
      "skills": [
        "./skills/blog-image-generator",
        "./skills/skill-writing"
      ]
    }
  ]
}
```

When `agents` or `skills` arrays are present, cmx uses them directly. Paths are resolved relative to the repository root.

#### Format 2: `source` path without explicit arrays

Used when each plugin lives in its own subdirectory and cmx should discover artifacts by walking (e.g. `anthropics/claude-code` bundled plugins):

```json
{
  "plugins": [
    {
      "name": "code-review",
      "description": "Automated code review",
      "source": "./plugins/code-review"
    },
    {
      "name": "commit-commands",
      "description": "Git commit workflows",
      "source": "./plugins/commit-commands"
    }
  ]
}
```

When no `agents`/`skills` arrays are present but `source` is a relative path, cmx resolves the path and walks the directory tree to discover `.md` agents and `SKILL.md` skills — the same logic used for repos without a marketplace.json.

#### Format 3: Remote `source` objects (not yet supported)

The official Claude Code plugin format also supports remote sources (`url`, `github`, `git-subdir`, `npm`), as seen in `anthropics/claude-plugins-official`:

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

cmx recognizes these entries and emits a warning that remote sources are not yet supported. Future versions may add support for fetching and scanning remote plugin sources.

### Scanning priority

1. If `.claude-plugin/marketplace.json` exists, cmx reads it and processes each plugin entry using the format rules above.
2. If no `marketplace.json` is present, cmx falls back to walking the entire tree for `.md` files with agent frontmatter and directories with `SKILL.md`.

### Compatibility with Claude Code plugins

This format is compatible with Claude Code's native plugin system. A marketplace registered with cmx can also be used directly via:

```
/plugin marketplace add <repo-url>
```

cmx adds version tracking, checksum verification, LLM-powered diff analysis, and cross-platform installation on top of the native plugin system.

## Source management

cmx tracks registered sources in `~/.config/context-mixer/sources.json`:

```json
{
  "version": 1,
  "sources": {
    "guidelines": {
      "type": "local",
      "path": "/Users/dev/guidelines"
    },
    "anthropic-skills": {
      "type": "git",
      "url": "https://github.com/anthropics/skills",
      "local_clone": "/Users/dev/.config/context-mixer/sources/anthropic-skills",
      "branch": "main",
      "last_updated": "2026-03-20T12:00:00Z"
    }
  }
}
```

Git-backed sources are auto-updated when stale (>60 minutes) during `browse`, `install`, and `outdated` operations.

## Lock file

One lock file per scope tracks installed state:

- **Global**: `~/.config/context-mixer/cmx-lock.json` — personal install manifest, not in any repo.
- **Local**: `.context-mixer/cmx-lock.json` — lives in the project, can be committed for team sharing.

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

- `version` — from frontmatter at install time (null if absent)
- `source_checksum` — checksum of the artifact in the source at install time
- `installed_checksum` — checksum of what was written to disk (initially matches source)

## Versioning and checksums

Artifact identity is tracked with two complementary mechanisms: **checksums** (automatic) and **versions** (optional frontmatter).

### Checksums

Checksums are always computed — no author effort required. SHA-256 over the artifact content — the single `.md` file for agents, or a deterministic hash of all files in the skill directory.

| Scenario | What checksum tells you |
|----------|------------------------|
| Installed artifact vs source | Has the source been updated since install? |
| Installed artifact vs its lock file entry | Has the local copy been hand-edited? |
| Two sources with the same artifact name | Are they actually identical? |

### Versions

An optional `version` field in the artifact frontmatter gives the author a way to communicate significance of changes.

| Scenario | What version tells you |
|----------|------------------------|
| Source has a newer version | Whether the update is a major/minor/patch change |
| Deciding whether to overwrite local edits | Whether the upstream jump is worth losing your tweaks |
| Multiple sources with the same artifact | Which is more recent, by the author's intent |

### How they work together

| Installed state | Source state | What cmx tells the user |
|-----------------|-------------|--------------------------|
| Checksum matches source | — | ✅ Up to date |
| Checksum differs, versions differ | 1.0 → 1.1 | ⚠️ Update available |
| Checksum differs, no versions | — | ⚠️ Source has changed |
| No lock entry, source has version | — | ⚠️ Untracked |
| Deprecated in source | — | ⛔ Deprecated |

**Design principle**: checksums are free and always present; versions are opt-in and add human-meaningful context.

## Platform paths

| Platform | Project agents | User agents | Project skills | User skills |
|----------|---------------|-------------|----------------|-------------|
| Claude Code | `.claude/agents/` | `~/.claude/agents/` | `.claude/skills/` | `~/.claude/skills/` |
| GitHub Copilot | `.github/agents/` | `~/.copilot/agents/` | `.github/skills/` | `~/.copilot/skills/` |
| Cursor | `.cursor/agents/` | `~/.cursor/agents/` | `.cursor/skills/` | `~/.cursor/skills/` |
| Windsurf | `.windsurf/agents/` | `~/.codeium/windsurf/agents/` | `.windsurf/skills/` | `~/.codeium/windsurf/skills/` |
| Gemini CLI | `.gemini/agents/` | `~/.gemini/agents/` | `.gemini/skills/` | `~/.gemini/skills/` |

---

## Commands (v2.0.0)

### `cmx source` — Manage marketplace sources

```
cmx source add <name> <path-or-url>    # Register a marketplace
cmx source list                         # List registered sources
cmx source browse <name>                # Show available artifacts
cmx source update [name]                # Fetch latest (all if no name)
cmx source remove <name>                # Unregister and clean up
```

### `cmx agent` / `cmx skill` — Manage artifacts

```
cmx agent install <name>                # Install from sources
cmx agent install <source>:<name>       # From a specific source
cmx agent install --all                 # Install all available
cmx agent install <name> --local        # Into project scope

cmx agent update <name>                 # Update from source
cmx agent update --all                  # Update all tracked

cmx agent list                          # List installed agents
cmx agent diff <name>                   # LLM-powered diff analysis

cmx skill install/update/list/diff      # Same for skills
```

### `cmx list` — Aggregate view

```
cmx list                                # All installed artifacts
```

Shows Name, Installed version, Source, Available version, and status indicators (✅ ⚠️ ⛔).

### `cmx outdated` — What needs attention

```
cmx outdated                            # Show outdated/untracked/deprecated
```

### `cmx config` — LLM configuration

```
cmx config show                         # Current settings
cmx config gateway <openai|ollama>      # Set LLM provider
cmx config model <name>                 # Set model (default: gpt-5.4)
```

---

## Future commands

| Command | Purpose |
|---------|---------|
| `cmx info <name>` | Detailed metadata for an installed artifact |
| `cmx uninstall <name>` | Remove with dependency warnings |
| `cmx check` | Verify integrity and content checksums |
| `cmx mix` | Merge aspects between craftsperson and local agents (requires LLM) |

---

*Draft started 2026-03-19. Updated 2026-03-20.*
