# cmx — Package Manager for Curated Agentic Context

cmx manages the lifecycle of **agents** and **skills** for AI coding assistants — versioning, installation, updates, and distribution.

## What are agents and skills?

| Artifact | Shape | Purpose |
|----------|-------|---------|
| **Agent** | Single `.md` file with YAML frontmatter | Curated guidance for a tech stack — applies across many repositories |
| **Skill** | Directory with `SKILL.md` + supporting files | Composable tool capability — task-specific functionality |

## What cmx does

- **Source management** — register git repositories or local directories as artifact sources (plugin marketplaces)
- **Install & update** — install agents and skills globally or per-project, track versions and checksums
- **Status tracking** — see what's installed, what's outdated, what's deprecated
- **LLM-powered diff** — use AI to analyze differences between installed and source versions
- **Cross-platform** — works with Claude Code, GitHub Copilot, Cursor, Windsurf, and Gemini CLI

## Quick example

```bash
# Add a source marketplace
cmx source add guidelines https://github.com/svetzal/guidelines

# Browse what's available
cmx source browse guidelines

# Install an agent globally
cmx agent install python-craftsperson

# Install all available agents
cmx agent install --all

# Check what needs updating
cmx outdated

# See an LLM-powered analysis of changes
cmx agent diff rust-craftsperson

# Update everything
cmx agent update --all
```
