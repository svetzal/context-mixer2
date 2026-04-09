# Changelog

All notable changes to cmx and cmf will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.5.1] - 2026-04-09

### Fixed

- Agent scanner no longer recurses into skill directories — `.md` reference files inside skills were being falsely detected as agents
- Agent scanner now requires `.md` files to live in an `agents/` directory to be recognized as agents, preventing false positives from documentation or other markdown files with similar frontmatter

## [2.5.0] - 2026-04-05

### Added

- **cmf (context mixer forge)** — new publisher/authoring tool for managing agentic context artifacts, shipped alongside cmx in the same distribution
  - `cmf status` — repo overview dashboard showing plugins, agents, skills, facets, validation summary
  - `cmf validate` — aggregate validation across plugins, marketplace, and facets
  - `cmf plugin list` — list all plugins with agent/skill counts per plugin
  - `cmf plugin init <name>` — scaffold new plugin directory with plugin.json, agents/, skills/
  - `cmf plugin validate` — check plugin structure and frontmatter integrity
  - `cmf marketplace validate` — check marketplace.json consistency against plugin directories
  - `cmf marketplace generate` — regenerate marketplace.json from plugin directory structure, preserving owner metadata and categories
  - `cmf facet list` — list facets grouped by category and recipes
  - `cmf facet validate` — validate facet frontmatter, scope fields, and recipe references
  - `cmf recipe list` — list available recipes with target paths
  - `cmf recipe assemble <name>` / `--all` — assemble agents from facets via naive concatenation
  - `cmf recipe diff <name>` — compare assembled output against current agent file
  - `cmf manifest generate` — generate multi-platform manifests for Codex, Cursor, and Gemini from Claude plugin sources

### Changed

- Converted project to Cargo workspace with `cmx` and `cmf` as separate binaries sharing the cmx library crate
- Unified versioning via `[workspace.package]` — both binaries share the same version
- Promoted `json_file` module from `pub(crate)` to `pub` for cross-crate use
- Release archives now include both `cmx` and `cmf` binaries
- Homebrew formula (`brew install svetzal/tap/cmx`) now installs both `cmx` and `cmf`
- mdbook documentation expanded with pages for plugins, facets, recipes, and cmf command reference

## [2.4.2] - 2026-03-28

### Fixed

- Show all sources when the same artifact exists in multiple registered repos

## [2.4.1] - 2026-03-27

### Fixed

- Show installed version from disk for untracked artifacts in `cmx list`

## [2.4.0] - 2026-03-27

### Added

- Support metadata-nested version extraction (`metadata.version` in frontmatter)

## [2.3.0] - 2026-03-25

### Added

- Display source repository version for skills in `cmx source browse`
- Gate `tokio` and `mojentic` behind optional `llm` feature for lean default builds

### Changed

- Refactored tests to eliminate knowledge duplication

### Security

- Updated `sha2` and transitive cryptographic dependencies
- Updated `uuid` to 1.23.0

## [2.2.0] - 2026-03-24

### Fixed

- Marketplace scanning now discovers agents and skills from plugins that use `source` paths without explicit `agents`/`skills` arrays (e.g. `anthropics/claude-code` bundled plugin format)
- Remote source objects (`url`, `github`, `git-subdir`, `npm`) now emit a clear warning instead of being silently ignored

## [2.1.1] - 2026-03-23

### Security

- Updated transitive dependency `iri-string` to 0.7.11 to address security vulnerabilities

## [2.1.0] - 2026-03-20

### Added

- `cmx search <keyword>` command — searches all registered sources for agents and skills by name and description
- mdbook documentation site deployed to GitHub Pages
- Artifact descriptions extracted from frontmatter for search matching

## [2.0.0] - 2026-03-20

### Added

- `cmx source add/list/browse/update/remove` for managing plugin marketplace sources
- `cmx agent install/update/list/diff` for managing agents
- `cmx skill install/update/list/diff` for managing skills
- `cmx list` aggregate view of all installed artifacts with status indicators (✅ ⚠️ ⛔)
- `cmx outdated` to show artifacts needing attention (untracked, changed, deprecated)
- `cmx config show/gateway/model` for LLM configuration
- `--all` flag for `install` and `update` commands
- `--local` flag for project-scoped installation
- Lock file tracking with SHA-256 checksums and version metadata
- LLM-powered diff analysis via mojentic (OpenAI and Ollama gateways)
- Plugin marketplace format support (`.claude-plugin/marketplace.json`)
- Fallback tree-walking scanner for repos without marketplace.json
- Auto-update for stale git-backed sources (>60 min)
- Deprecation support in frontmatter (`deprecated`, `deprecated_reason`, `deprecated_replacement`)
- Versioning support in frontmatter with semver
- Source cleanup on remove (deletes cloned git repos)
- GitHub Actions CI (fmt, clippy, tests) and release pipeline
- Homebrew tap distribution via `brew tap svetzal/tap && brew install cmx`
- Cross-platform builds (macOS ARM64, macOS x64, Linux x64)
