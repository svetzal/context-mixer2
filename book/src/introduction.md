# Context Mixer — Curated Agentic Context

Context Mixer is three command-line tools that manage the lifecycle of curated
agentic context — agents, skills, and the engineering intents behind them —
across AI coding assistants.

| Tool | Role |
| --- | --- |
| **cmx** | The package manager: installs, versions, updates, and reconciles agents and skills across every platform you use |
| **cmf** (Context Mixer Forge) | The compiler: assembles a profile-specific slice of an intent atlas into an installed agent or skill, and records exactly what it compiled |
| **cmv** (Context Mixer Verify) | The verifier: checks, deterministically and from source alone, that a repository holds the intents cmf compiled for it |

## The intent atlas

cmf and cmv work from an **intent atlas**: an externally maintained repository
of falsifiable engineering intents, organized by the ecosystems they apply to,
with the validators that check each intent beside it and the sensors that
recognize each ecosystem in a project. The reference atlas is
[svetzal/guidelines](https://github.com/svetzal/guidelines), one person's body
of knowledge about how software should be built. Fork it or replace it with
your own; the tooling carries no opinions of its own and verifies any atlas the
same way. The loop is walked once in
[Compiling and Verifying Intents](./guide/compiling-and-verifying.md), and its
reasoning is in
[Deterministic Verification of Intents](./design/deterministic-verification.md).

## cmx: the package manager

cmx manages the lifecycle of **agents** and **skills** for AI coding assistants — versioning, installation, updates, and distribution.

### What are agents and skills?

| Artifact | Shape | Purpose |
|----------|-------|---------|
| **Agent** | Single `.md` file with YAML frontmatter | Curated guidance for a tech stack — applies across many repositories |
| **Skill** | Directory with `SKILL.md` + supporting files | Composable tool capability — task-specific functionality |

### What cmx does

- **Source management** — register git repositories or local directories as artifact sources (plugin marketplaces)
- **Install & update** — install agents and skills globally or per-project, across the platforms you use, tracking versions and checksums
- **Status tracking** — see what's installed, what's outdated, what's deprecated; `cmx doctor` surveys the whole system
- **Reconcile** — promote in-place edits back to a canonical home, and sync a skill that has diverged across tools
- **Sets** — group installed artifacts into named, activatable sets so you can switch off the standing context cost of unrelated work without losing track of it
- **LLM-powered diff** — use AI to analyze differences between installed and source versions, directionally
- **Cross-platform** — works with Claude Code, GitHub Copilot, Cursor, Windsurf, Gemini CLI, opencode, Codex CLI, Pi, Crush, Amp, Zed, OpenHands, Hermes, and Devin

### Quick example

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

## Where next

- [Installation](./getting-started/installation.md) and the [Quick Start](./getting-started/quick-start.md) for cmx.
- [Compiling and Verifying Intents](./guide/compiling-and-verifying.md) for the cmf and cmv loop.
- [Writing Intents](./creating/intents.md) to author an atlas, including validators and sensors.
- Command references for [cmx](./reference/commands.md), [cmf](./reference/cmf-commands.md), and [cmv](./reference/cmv-commands.md).
