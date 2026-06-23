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

By default — with no `--platform` — cmx installs to **every platform already in
use**, i.e. those with tracked artifacts at the target scope. A new install
joins the tools you actually run (e.g. Claude + Codex + Hermes) and stays in sync
across them; when nothing is tracked yet it falls back to Claude. Each landing is
reported on its own line, naming the platform.

Use `--platform` to constrain the install to one tool (which also onboards a new
one):

```bash
cmx agent install python-craftsperson --platform cursor
cmx skill install skill-creator --platform windsurf --local
```

You can also set the platform via environment variable:

```bash
export CMX_PLATFORM=copilot
cmx agent install python-craftsperson
```

Supported platform values: `claude`, `copilot`, `cursor`, `windsurf`, `gemini`,
`opencode`, `codex`, `pi`, `crush`, `amp`, `zed`, `openhands`, `hermes`.

To make the in-use set explicit rather than inferred, declare a managed set with
[`cmx config platforms`](../reference/commands.md#managed-platforms). See
[Platform Paths](../reference/platforms.md) for the full directory table.

## What happens on install

1. The artifact is copied to the appropriate platform directory
2. A SHA-256 checksum is computed
3. The version (if present in frontmatter) is recorded
4. An entry is written to the platform's lock file with source, checksum, version, and timestamp
