# Sources

Sources are plugin marketplace repositories — git repos or local directories containing agents and skills.

## Adding sources

```bash
# Git repository (cloned automatically)
cmx source add team https://github.com/acme/agent-standards

# Local directory
cmx source add guidelines ~/Work/guidelines
```

## Listing sources

```bash
cmx source list
```

## Browsing a source

```bash
cmx source browse guidelines
```

Shows all agents and skills with their versions. Deprecated artifacts are marked with ⛔.

## Updating sources

Git-backed sources are automatically updated when stale (>60 minutes since last fetch) during `browse`, `install`, and `outdated` operations.

To explicitly update:

```bash
# Update a specific source
cmx source update guidelines

# Update all git-backed sources
cmx source update
```

## Removing sources

```bash
cmx source remove guidelines
```

For git-backed sources, this also deletes the local clone.

## Marketplace format

cmx supports two source formats:

1. **Marketplace repos** — contain `.claude-plugin/marketplace.json` listing plugins with their agents and skills. Compatible with Claude Code's native plugin system.

2. **Simple repos** — any directory with `.md` agent files and skill directories containing `SKILL.md`. cmx auto-discovers these by walking the tree.

See [Marketplace Structure](../creating/marketplace.md) for details on creating your own.
