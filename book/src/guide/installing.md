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

By default, artifacts install globally (`~/.claude/agents/` or `~/.claude/skills/`).

Use `--local` to install into the current project:

```bash
cmx agent install python-craftsperson --local
```

This installs to `.claude/agents/` in the current directory.

## What happens on install

1. The artifact is copied to the appropriate platform directory
2. A SHA-256 checksum is computed
3. The version (if present in frontmatter) is recorded
4. An entry is written to the lock file with source, checksum, version, and timestamp
