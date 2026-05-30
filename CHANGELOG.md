# Changelog

All notable changes to cmx and cmf will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `cmx doctor` — a read-only survey of the whole system installation across every supported platform. It cross-references each platform's install directories and per-platform lock files and classifies every artifact as `tracked`, `drifted` (locally edited after install), `orphaned` (on disk but untracked — e.g. hand-authored skills), or `missing` (in a lock file but gone from disk), and flags artifacts duplicated across distinct install locations. Skills in the shared `.agents/skills` directory are reported once for the whole cohort rather than once per tool. `cmx doctor --local` also includes project scope. Exits non-zero (`2`) when drift, orphans, or missing entries are found, so it can gate a hook or CI check.
- `cmx::platform::Platform::ALL` — the exhaustive slice of platform variants, so cross-platform operations (like the survey) automatically cover every platform.
- `ConfigPaths::with_platform` — derive a path view bound to a different platform from a single base, reusing all platform-aware path resolution.

## [2.6.0] - 2026-05-29

### Added

- `--platform` global flag (and `CMX_PLATFORM` env var) for selecting the target AI coding assistant: `claude` (default), `copilot`, `cursor`, `windsurf`, `gemini`. All install, uninstall, update, list, outdated, info, and search commands respect the platform setting.
- Platform-aware install paths: agents and skills now install to the correct directory for each platform (e.g. `.cursor/agents/` for Cursor, `~/.codeium/windsurf/skills/` for Windsurf globally).
- Per-platform lock files: non-Claude platforms use `cmx-lock-<platform>.json` so installations for different tools remain independent. Claude keeps `cmx-lock.json` for backward compatibility.
- `cmx::platform::Platform` is now a public type in the `cmx` crate; `cmf` imports it from there rather than defining its own copy.
- `cmf manifest generate` now emits `.windsurf-plugin/` manifests, so marketplaces built with cmf no longer silently exclude Windsurf users.
- Three additional `--platform` targets: `opencode`, `codex`, and `pi`. Skills for all three install to the shared cross-tool `.agents/skills/` (project) and `~/.agents/skills/` (user) convention that opencode, Codex, and Pi all read.
- opencode agents install as markdown to `.opencode/agent/` (project) and `~/.config/opencode/agent/` (user).
- Codex agents are transformed from cmx markdown into Codex subagent TOML (`<name>.toml`) on install, mapping `name`, `description`, the markdown body (`developer_instructions`), and an optional `model` field. Installed to `.codex/agents/` / `~/.codex/agents/`.
- Per-platform support gating: platforms declare which artifact kinds they support. Pi supports skills only, so `cmx agent install --platform pi` (and uninstall/update) fails with a clear, actionable error rather than installing into a directory Pi never reads.
- Five additional skills-only `--platform` targets: `crush`, `amp`, `zed`, `openhands`, and `hermes`. All consume the cross-tool `.agents/skills/` standard, so a single skill install serves the whole cohort (plus opencode/codex/pi) at once. None has a file-droppable agent concept, so `cmx agent install` for these fails with a clear error. Two have user-scope path nuances: Amp resolves user skills under `~/.config/agents/skills/` (XDG), and Hermes under `~/.hermes/skills/` (its global source of truth).

### Notes

- opencode, Codex, and Pi have no Claude-style plugin/marketplace manifest format, so `cmf manifest generate` intentionally does not emit manifest directories for them.
- Because opencode/Codex/Pi share the `.agents/skills/` directory, uninstalling a skill under one of these platforms removes it for all tools that read `.agents/`.

### Changed

- Renamed the `Codex` platform variant to `Copilot` and its generated manifest directory from `.codex-plugin/` to `.copilot-plugin/`, matching the documented platform name (GitHub Copilot). Re-run `cmf manifest generate` to refresh manifest directories.

### Fixed

- `cmx agent install` and `cmx skill install` now roll back a freshly copied artifact when the lockfile write fails, eliminating the ghost-install state where an artifact exists on disk with no lockfile entry
- `json_file::save_json` now writes atomically via a sibling `.tmp` file followed by a rename, preventing partial writes from corrupting an existing JSON file

## [2.5.3] - 2026-04-11

### Changed

- Extracted `find_entry_with` helper in lockfile module for reusable lock entry lookup across scopes
- Extracted `split_frontmatter_str` helper in scan module to DRY up frontmatter parsing
- Refactored `update_with` in install module to use the new `find_entry_with` helper

## [2.5.2] - 2026-04-10

### Fixed

- `cmx list` now only shows the installed version on the row matching the source from which the artifact was actually installed, leaving the column blank for other sources offering the same artifact
- Disambiguated "not installed from this source" (blank) from "installed but unversioned" (`-`) in the Installed column

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
