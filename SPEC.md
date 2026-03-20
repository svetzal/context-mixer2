# cmx — Package Manager for Curated Agentic Context

> Working draft. Expect iteration.

## What cmx does

cmx manages the lifecycle of **craftsperson agents** and **agent skills** — versioning, dependencies, installation, and distribution across AI coding assistants.

**Key distinction from hone**: hone derives ad-hoc agents from a repo's existing structure. cmx curates, distributes, and manages polished craftsperson agents and composable skills.

## Artifact types

| Artifact | Shape | Purpose |
|----------|-------|---------|
| **Agent** | Single `.md` file with YAML frontmatter | Portable, curated guidance for a tech stack — applies across many repositories |
| **Skill** | Directory with `SKILL.md` + supporting files | Composable tool capability — task-specific functionality the agent can invoke |

Agents and skills can be installed globally (`~/.claude/agents/`, `~/.claude/skills/`) or locally in a project (`.claude/agents/`, `.claude/skills/`).

### Detection rules

| Signal | Type |
|--------|------|
| Single `.md` file with YAML frontmatter containing `name` and `description` | Agent |
| Directory containing `SKILL.md` with YAML frontmatter | Skill |

### Agent frontmatter

```yaml
---
name: python-craftsperson
description: |
  Use this agent when writing, reviewing, or maintaining Python code.
model: sonnet
---
```

Optional package management fields (backward compatible): `version`, `type`, `author`, `license`, `scopes`, `dependencies`, `platforms`.

### Skill directory structure

```
my-skill/
├── SKILL.md              # Required: frontmatter + instructions
├── scripts/              # Optional: deterministic automation
├── references/           # Optional: detailed documentation
└── examples/             # Optional: templates, samples
```

## Source repositories

The primary distribution mechanism is the **source repository** — a git repo containing curated agents and skills. Teams share a source repo; individuals maintain their own.

```
guidelines/
├── agents/
│   ├── python-craftsperson.md
│   ├── typescript-craftsperson.md
│   └── ...
├── skills/
│   ├── skill-writing/
│   │   └── SKILL.md
│   └── ...
└── ...
```

cmx tracks registered source repos in `~/.context-mixer/sources.json`:

```json
{
  "version": 1,
  "sources": {
    "guidelines": {
      "type": "local",
      "path": "/Users/dev/guidelines",
      "remote": "https://github.com/svetzal/guidelines"
    },
    "team-standards": {
      "type": "git",
      "url": "https://github.com/acme/agent-standards",
      "local_clone": "/Users/dev/.context-mixer/sources/team-standards",
      "branch": "main"
    }
  }
}
```

Source repos are scanned for artifacts by walking the tree and matching detection rules. A conventional `agents/` + `skills/` structure is recommended but not required.

## Lock file

One lock file per scope tracks installed state:

- Global: `~/.context-mixer/cmx-lock.json`
- Local: `.context-mixer/cmx-lock.json`

```json
{
  "version": 1,
  "packages": {
    "python-craftsperson": {
      "type": "agent",
      "version": "1.0.0",
      "installed_at": "2026-03-19T10:30:00Z",
      "source": {
        "type": "source-repo",
        "repo": "guidelines",
        "path": "agents/python-craftsperson.md"
      },
      "checksum": "sha256:abc123..."
    }
  }
}
```

## Platform paths

| Platform | Project agents | User agents | Project skills | User skills |
|----------|---------------|-------------|----------------|-------------|
| Claude Code | `.claude/agents/` | `~/.claude/agents/` | `.claude/skills/` | `~/.claude/skills/` |
| GitHub Copilot | `.github/agents/` | `~/.copilot/agents/` | `.github/skills/` | `~/.copilot/skills/` |
| Cursor | `.cursor/agents/` | `~/.cursor/agents/` | `.cursor/skills/` | `~/.cursor/skills/` |
| Windsurf | `.windsurf/agents/` | `~/.codeium/windsurf/agents/` | `.windsurf/skills/` | `~/.codeium/windsurf/skills/` |
| Gemini CLI | `.gemini/agents/` | `~/.gemini/agents/` | `.gemini/skills/` | `~/.gemini/skills/` |

---

## First commands (v0.1.0)

### `cmx source` — Manage source repositories

```
cmx source add <name> <path-or-url>    # Register a source repo
cmx source list                         # List registered sources
cmx source browse <name>                # Show available artifacts in a source
cmx source pull <name>                  # Pull latest for git-backed sources
cmx source remove <name>                # Unregister (does not delete artifacts)
```

Examples:
```
$ cmx source add guidelines ~/Work/Projects/Personal/guidelines
Source 'guidelines' registered: 16 agents, 2 skills found.

$ cmx source add team https://github.com/acme/agent-standards
Cloning to ~/.context-mixer/sources/team...
Source 'team' registered: 4 agents, 3 skills found.

$ cmx source browse guidelines
Agents:
  python-craftsperson
  typescript-craftsperson
  rust-craftsperson
  ... (13 more)
Skills:
  skill-writing
```

### `cmx install` — Install an artifact

```
cmx install <name>                      # Resolve from registered sources
cmx install <source>:<name>             # From a specific source
cmx install <path>                      # From a direct path
cmx install <name> --local              # Into project scope (default: global)
```

Examples:
```
$ cmx install python-craftsperson
Installed python-craftsperson (agent) from 'guidelines' -> ~/.claude/agents/

$ cmx install guidelines:stride-claiming-tasks --local
Installed stride-claiming-tasks (skill) from 'guidelines' -> .claude/skills/
```

On install, cmx:
1. Detects artifact type (agent or skill)
2. Copies to the appropriate platform path
3. Records source, checksum, and timestamp in the lock file

### `cmx list` — See what's installed

```
cmx list                                # All installed artifacts
cmx list --agents                       # Agents only
cmx list --skills                       # Skills only
cmx list --scope global|local           # Filter by scope
```

Example:
```
$ cmx list
Global agents:
  python-craftsperson      (from guidelines)
  typescript-craftsperson   (from guidelines)

Global skills:
  stride-claiming-tasks    (from guidelines)

Local agents:
  (none)

Local skills:
  (none)
```

---

## Future commands (not yet)

| Command | Purpose |
|---------|---------|
| `cmx info <name>` | Detailed metadata for an installed package |
| `cmx uninstall <name>` | Remove with dependency warnings |
| `cmx update [name\|--all]` | Update from source with modification detection |
| `cmx check` | Verify dependency integrity and content checksums |
| `cmx mix` | Merge aspects between craftsperson and local agents (requires LLM) |

## Open questions

- Should `cmx source add` auto-discover artifacts anywhere in the tree, or require conventional `agents/` + `skills/` structure?
- How to handle artifact name collisions across multiple sources? Require explicit `source:name`?
- Should `cmx install` install to all detected platforms or just one?
- Should the lock file live in `~/.context-mixer/` or alongside the platform paths (`~/.claude/`)?
- How to handle existing manually-installed agents/skills? Auto-adopt into lock file?

---

*Draft started 2026-03-19.*
