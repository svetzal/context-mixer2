# Context Mixer — Project Charter

## Purpose

Context Mixer manages the lifecycle of curated agentic context — portable agent definitions and composable skills — across AI coding assistants. It ships as two complementary CLIs:

- **cmx** — the consumer tool: a package manager that installs, versions, updates, and reconciles agents and skills across platforms.
- **cmf** (Context Mixer Forge) — the publisher tool: authoring support for the material cmx consumes — facets assembled into agents by recipes, plugin scaffolding and validation, and marketplace/manifest generation.

The project rests on two pillars of equal weight:

1. **Marketplace distribution.** Git-backed plugin marketplaces with a standard manifest format are the distribution mechanism for published, shareable artifacts — versioned, checksummed, and tracked through install, update, and deprecation.
2. **Cross-platform curation and reconciliation.** A tool-neutral canonical home holds hand-authored private artifacts; cmx projects them to every platform in use and keeps the copies honest — detecting drift, promoting in-place edits back to the canonical copy, and syncing diverged copies across platforms. This lifecycle matters because assistants edit their own installed skills: curate once, project to many, reconcile what drifts.

## Goals

- Provide two focused CLIs — cmx to consume and manage artifacts, cmf to author and publish them
- Install, update, and track agents and skills across Claude Code, GitHub Copilot, Cursor, Windsurf, Gemini CLI, opencode, Codex CLI, Pi, Crush, Amp, Zed, OpenHands, and Hermes
- Support both global (user-wide) and local (project-scoped) installation with lock file tracking
- Enable plugin marketplaces as git repositories with a standard manifest format
- Track artifact integrity via SHA-256 checksums and optional semver versioning
- Surface outdated, untracked, deprecated, and diverged artifacts clearly
- Provide a tool-neutral canonical home for hand-authored private artifacts, with a full reconciliation lifecycle: a system-wide survey (`doctor`) that diagnoses a disorganized installation, adoption of orphaned artifacts, promotion of in-place edits back to the canonical copy, and synchronization of copies that have diverged across platforms — so a curated set survives both day-to-day assistant edits and migrating between coding assistants
- Support publishers with authoring tooling: facets composed into agents by recipes, plugin and marketplace validation, and generated multi-platform manifests
- Offer LLM-powered diff analysis for understanding changes between installed and source versions

## Non-Goals

- Deriving agents from a repository's existing structure or code (that is what hone does). cmf's recipe assembly is deterministic composition of hand-curated facets — authoring support, not inference or generation
- Hosting a centralized registry or marketplace service
- Managing LLM API keys, billing, or model routing
- Replacing the native plugin systems of supported coding assistants — cmx layers version tracking and cross-platform management on top of them

## Target Users

Software developers who use AI coding assistants and want to maintain a curated, version-tracked set of agents and skills across projects and tools — and who may also publish that curated context for others through plugin marketplaces.
