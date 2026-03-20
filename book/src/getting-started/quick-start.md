# Quick Start

## 1. Add a source

Sources are git repositories (or local directories) that contain agents and skills. Register one:

```bash
# From a git URL
cmx source add guidelines https://github.com/svetzal/guidelines

# Or from a local directory
cmx source add my-agents ~/Work/my-agents
```

You can also add Anthropic's official skills:

```bash
cmx source add anthropic-skills https://github.com/anthropics/skills
```

## 2. Browse available artifacts

```bash
cmx source browse guidelines
```

This shows all agents and skills in the source, with versions and deprecation status.

## 3. Install artifacts

```bash
# Install a single agent
cmx agent install python-craftsperson

# Install from a specific source
cmx agent install guidelines:python-craftsperson

# Install all available agents
cmx agent install --all

# Install into the current project (instead of globally)
cmx agent install python-craftsperson --local
```

## 4. Check status

```bash
# See everything installed
cmx list

# Check what's outdated
cmx outdated
```

## 5. Update

```bash
# Update a specific agent
cmx agent update python-craftsperson

# Update all tracked agents
cmx agent update --all
```

## Where artifacts get installed

By default, cmx installs to Claude Code's global directories:

| Type | Global | Local |
|------|--------|-------|
| Agents | `~/.claude/agents/` | `.claude/agents/` |
| Skills | `~/.claude/skills/` | `.claude/skills/` |
