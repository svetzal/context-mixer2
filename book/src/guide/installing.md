# Installing Artifacts

## Install a single artifact

```bash
cmx agent install python-craftsperson
cmx skill install skill-creator
```

If the artifact exists in multiple sources, cmx will ask you to disambiguate:

```bash
cmx agent install guidelines:python-craftsperson
```

## Install all available

```bash
cmx agent install --all
cmx skill install --all
```

This installs every artifact from registered sources that isn't already tracked in the lock file with a matching version.

## Local vs global install

By default, artifacts install globally (e.g. `~/.claude/agents/` or `~/.claude/skills/` for Claude Code).

Use `--local` to install into the current project:

```bash
cmx agent install python-craftsperson --local
```

This installs to the platform's project-scoped directory (e.g. `.claude/agents/` for Claude Code).

## Choosing a platform

By default, cmx installs to Claude Code paths. Use `--platform` to target a different tool:

```bash
cmx agent install python-craftsperson --platform cursor
cmx skill install skill-creator --platform windsurf --local
```

You can also set the platform via environment variable:

```bash
export CMX_PLATFORM=copilot
cmx agent install python-craftsperson
```

Supported platform values: `claude` (default), `copilot`, `cursor`, `windsurf`, `gemini`.

See [Platform Paths](../reference/platforms.md) for the full directory table.

## What happens on install

1. The artifact is copied to the appropriate platform directory
2. A SHA-256 checksum is computed
3. The version (if present in frontmatter) is recorded
4. An entry is written to the platform's lock file with source, checksum, version, and timestamp
