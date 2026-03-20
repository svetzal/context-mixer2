# Platform Paths

cmx currently installs to Claude Code directories by default. Future versions will support additional platforms.

## Supported platforms

| Platform | Project agents | User agents | Project skills | User skills |
|----------|---------------|-------------|----------------|-------------|
| Claude Code | `.claude/agents/` | `~/.claude/agents/` | `.claude/skills/` | `~/.claude/skills/` |
| GitHub Copilot | `.github/agents/` | `~/.copilot/agents/` | `.github/skills/` | `~/.copilot/skills/` |
| Cursor | `.cursor/agents/` | `~/.cursor/agents/` | `.cursor/skills/` | `~/.cursor/skills/` |
| Windsurf | `.windsurf/agents/` | `~/.codeium/windsurf/agents/` | `.windsurf/skills/` | `~/.codeium/windsurf/skills/` |
| Gemini CLI | `.gemini/agents/` | `~/.gemini/agents/` | `.gemini/skills/` | `~/.gemini/skills/` |

## Current default

cmx v2.0.0 installs to Claude Code paths only. The agent and skill formats follow the [Agent Skills specification](https://agentskills.io/specification), which is supported by all platforms listed above.
