# cmx — Project Charter

## Purpose

cmx is a package manager for curated agentic context (agents and skills) across AI coding assistants. It manages the lifecycle of portable agent definitions and composable skills — versioning, installation, updates, and distribution — using git-backed plugin marketplaces as the distribution mechanism.

## Goals

- Provide a single CLI to install, update, and track agents and skills across Claude Code, GitHub Copilot, Cursor, Windsurf, and Gemini CLI
- Support both global (user-wide) and local (project-scoped) installation with lock file tracking
- Enable plugin marketplaces as git repositories with a standard manifest format
- Track artifact integrity via SHA-256 checksums and optional semver versioning
- Surface outdated, untracked, and deprecated artifacts clearly
- Offer LLM-powered diff analysis for understanding changes between installed and source versions

## Non-Goals

- Generating or deriving agents from repository structure (that is what hone does)
- Hosting a centralized registry or marketplace service
- Managing LLM API keys, billing, or model routing
- Replacing the native plugin systems of supported coding assistants — cmx layers version tracking and cross-platform management on top of them

## Target Users

Software developers who use AI coding assistants and want to maintain a curated, version-tracked set of agents and skills across projects and tools.
