# Platform Paths

cmx can install agents and skills to any supported AI coding assistant platform.
Use the `--platform` flag or the `CMX_PLATFORM` environment variable to choose a target.
The default platform is Claude Code, which preserves backward compatibility.

## Selecting a platform

```bash
# Install to Cursor globally
cmx agent install python-craftsperson --platform cursor

# Install to Windsurf locally
cmx skill install skill-creator --platform windsurf --local

# Set the platform via environment variable
export CMX_PLATFORM=copilot
cmx agent install python-craftsperson
```

The `--platform` flag is global — it applies to all subcommands.

## Supported platforms

| Platform | Value | Project agents | User agents | Project skills | User skills |
|----------|-------|---------------|-------------|----------------|-------------|
| Claude Code | `claude` (default) | `.claude/agents/` | `~/.claude/agents/` | `.claude/skills/` | `~/.claude/skills/` |
| GitHub Copilot | `copilot` | `.github/agents/` | `~/.copilot/agents/` | `.github/skills/` | `~/.copilot/skills/` |
| Cursor | `cursor` | `.cursor/agents/` | `~/.cursor/agents/` | `.cursor/skills/` | `~/.cursor/skills/` |
| Windsurf | `windsurf` | `.windsurf/agents/` | `~/.codeium/windsurf/agents/` | `.windsurf/skills/` | `~/.codeium/windsurf/skills/` |
| Gemini CLI | `gemini` | `.gemini/agents/` | `~/.gemini/agents/` | `.gemini/skills/` | `~/.gemini/skills/` |
| opencode | `opencode` | `.opencode/agent/` | `~/.config/opencode/agent/` | `.agents/skills/` | `~/.agents/skills/` |
| Codex CLI | `codex` | `.codex/agents/` ¹ | `~/.codex/agents/` ¹ | `.agents/skills/` | `~/.agents/skills/` |
| Pi | `pi` | *(not supported)* ² | *(not supported)* ² | `.agents/skills/` | `~/.agents/skills/` |

¹ **Codex agents are TOML, not markdown.** cmx agents are markdown files with
YAML frontmatter; the Codex CLI defines subagents as standalone TOML files. When
you install an agent with `--platform codex`, cmx transforms the source markdown
into a Codex subagent document (`<name>.toml`) with `name`, `description`,
`developer_instructions` (the markdown body), and an optional `model` field.

² **Pi has no native agent concept.** `cmx agent install --platform pi` fails
with a clear error. Pi supports skills only.

## The shared `.agents` skills convention

opencode, Codex, and Pi all read skills from the cross-tool `.agents/skills/`
location (project) and `~/.agents/skills/` (user), so cmx installs skills there
for all three. This is a **shared directory**: a skill installed under one of
these platforms is visible to the others.

A practical consequence: because each platform keeps its own lock file (below),
uninstalling a skill with one of these platforms removes the underlying
`.agents/skills/<name>` directory for *all* tools that read it. Reinstall it
under the other platform if you only meant to stop tracking it for one.

## Per-platform lock files

Each platform maintains its own lock file so installations for different tools
do not interfere with each other.

| Platform | Global lock file | Local lock file |
|----------|-----------------|-----------------|
| Claude Code | `~/.config/context-mixer/cmx-lock.json` | `.context-mixer/cmx-lock.json` |
| Copilot | `~/.config/context-mixer/cmx-lock-copilot.json` | `.context-mixer/cmx-lock-copilot.json` |
| Cursor | `~/.config/context-mixer/cmx-lock-cursor.json` | `.context-mixer/cmx-lock-cursor.json` |
| Windsurf | `~/.config/context-mixer/cmx-lock-windsurf.json` | `.context-mixer/cmx-lock-windsurf.json` |
| Gemini CLI | `~/.config/context-mixer/cmx-lock-gemini.json` | `.context-mixer/cmx-lock-gemini.json` |
| opencode | `~/.config/context-mixer/cmx-lock-opencode.json` | `.context-mixer/cmx-lock-opencode.json` |
| Codex CLI | `~/.config/context-mixer/cmx-lock-codex.json` | `.context-mixer/cmx-lock-codex.json` |
| Pi | `~/.config/context-mixer/cmx-lock-pi.json` | `.context-mixer/cmx-lock-pi.json` |

Claude Code keeps `cmx-lock.json` (no suffix) for backward compatibility with
installations made before platform selection was introduced.
