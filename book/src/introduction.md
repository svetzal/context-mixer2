# cmx — Package Manager for Curated Agentic Context

cmx manages the lifecycle of **agents** and **skills** for AI coding assistants — versioning, installation, updates, and distribution.

## What are agents and skills?

| Artifact | Shape | Purpose |
|----------|-------|---------|
| **Agent** | Single `.md` file with YAML frontmatter | Curated guidance for a tech stack — applies across many repositories |
| **Skill** | Directory with `SKILL.md` + supporting files | Composable tool capability — task-specific functionality |

## What cmx does

- **Source management** — register git repositories or local directories as artifact sources (plugin marketplaces)
- **Install & update** — install agents and skills globally or per-project, across the platforms you use, tracking versions and checksums
- **Status tracking** — see what's installed, what's outdated, what's deprecated; `cmx doctor` surveys the whole system
- **Reconcile** — promote in-place edits back to a canonical home, and sync a skill that has diverged across tools
- **Sets** — group installed artifacts into named, activatable sets so you can switch off the standing context cost of unrelated work without losing track of it
- **LLM-powered diff** — use AI to analyze differences between installed and source versions, directionally
- **Cross-platform** — works with Claude Code, GitHub Copilot, Cursor, Windsurf, Gemini CLI, opencode, Codex CLI, Pi, Crush, Amp, Zed, OpenHands, Hermes, and Devin

## Quick example

```bash
# Add a source marketplace
cmx source add guidelines https://github.com/svetzal/guidelines

# Search across all sources
cmx search python

# Browse a specific source
cmx source browse guidelines

# Install an agent globally
cmx agent install python-craftsperson

# Install all available agents
cmx agent install --all

# Group installed artifacts into a set you can switch off later
cmx set create rust-work
cmx set add rust-work rust-craftsperson

# Check what needs updating
cmx outdated

# See an LLM-powered analysis of changes
cmx agent diff rust-craftsperson

# Update everything
cmx agent update --all
```
