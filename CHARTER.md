# Context Mixer — Project Charter

## Purpose

Context Mixer manages the lifecycle of curated agentic context — portable agent definitions and composable skills — across AI coding assistants. It ships as three complementary CLIs:

- **cmx** — the consumer tool: a package manager that installs, versions, updates, and reconciles agents and skills across platforms.
- **cmf** (Context Mixer Forge) — the materializer: it reads an externally maintained intent atlas — the ecosystems it supports and their sensors, structured intent records in and around those ecosystems, and the validators beside them — selects and assembles a profile-specific slice for the project's ecosystems, installs the resulting agent or skill through cmx-core, and records what it composed.
- **cmv** (Context Mixer Verify) — the verifier: it checks, deterministically, that a repository holds the intents cmf compiled for it, by running each intent's codified validator from the atlas at the pinned revision.

The project rests on two pillars of equal weight:

1. **Marketplace distribution.** Git-backed plugin marketplaces with a standard manifest format are the distribution mechanism for published, shareable artifacts — versioned, checksummed, and tracked through install, update, and deprecation.
2. **Cross-platform curation and reconciliation.** A tool-neutral canonical home holds hand-authored private artifacts; cmx projects them to every platform in use and keeps the copies honest — detecting drift, promoting in-place edits back to the canonical copy, and syncing diverged copies across platforms. This lifecycle matters because assistants edit their own installed skills: curate once, project to many, reconcile what drifts. Consumer-side **sets** extend this pillar: named, user-composed groups of installed artifacts that can be activated and deactivated together, so the standing context cost of an installation can be managed without losing track of what belongs together.

## Goals

- Provide three focused CLIs — cmx to consume and manage artifacts, cmf to materialize intents into installed guidance, cmv to verify that a repository holds them
- Install, update, and track agents and skills across Claude Code, GitHub Copilot, Cursor, Windsurf, Gemini CLI, opencode, Codex CLI, Pi, Crush, Amp, Zed, OpenHands, Hermes, and Devin
- Support both global (user-wide) and local (project-scoped) installation with lock file tracking
- Enable plugin marketplaces as git repositories with a standard manifest format
- Track artifact integrity via SHA-256 checksums and optional semver versioning
- Surface outdated, untracked, deprecated, and diverged artifacts clearly
- Provide a tool-neutral canonical home for hand-authored private artifacts, with a full reconciliation lifecycle: a system-wide survey (`doctor`) that diagnoses a disorganized installation, adoption of orphaned artifacts, promotion of in-place edits back to the canonical copy, and synchronization of copies that have diverged across platforms — so a curated set survives both day-to-day assistant edits and migrating between coding assistants
- Support explainable, context-budgeted materialization of structured intents into agent and skill delivery surfaces
- Verify, from source alone, that a repository holds the intents its guidance was compiled from
- Offer LLM-powered diff analysis for understanding changes between installed and source versions

## Non-Goals

- Deriving guidance from a repository's existing structure or code (that is what hone does). cmf compiles explicitly authored intent records; it does not infer policy from a codebase
- Running a live guidance-selection harness. cmf may produce data and artifacts for dynamic harnesses, but session-time sensing, injection, leasing, and retraction belong to the harness
- Authoring, validating, or maintaining the intent atlas, including the sensors and validators that live beside its records. A separate tool owns those TOML documents and scripts; cmf and cmv consume them read-only
- Judging adherence with a model. cmv parses; it never asks
- Publishing plugins, marketplaces, or platform manifests. cmf installs assembled artifacts directly through cmx-core
- Hosting a centralized registry or marketplace service
- Managing LLM API keys, billing, or model routing
- Replacing the native plugin systems of supported coding assistants — cmx layers version tracking and cross-platform management on top of them

## Target Users

Software developers who use AI coding assistants and want to maintain a curated, version-tracked set of agents and skills across projects and tools — and who may also publish that curated context for others through plugin marketplaces.
