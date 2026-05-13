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

Claude Code keeps `cmx-lock.json` (no suffix) for backward compatibility with
installations made before platform selection was introduced.
