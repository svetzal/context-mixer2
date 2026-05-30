# cmx — Context Mixer

Why this project exists and what problem does it solve: @CHARTER.md

A package manager for curated agentic context (agents and skills), written in Rust.

## Quality Gates

MANDATORY pre-commit quality checks — run ALL before committing:

```bash
# Default build (lean, no LLM)
cargo fmt --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test && \
cargo deny check

# Full build (with LLM diff support)
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features
```

Additional recommended checks:

```bash
cargo audit          # security vulnerability scanning
cargo tarpaulin      # code coverage (target >80%)
```

- `--all-targets` is mandatory (ensures examples, tests, and benchmarks are checked)
- If any check fails, STOP immediately, fix the root cause (don't suppress), and re-run all checks
- Never use `#[allow(clippy::lint_name)]` without documenting why
- The `llm` feature gates tokio and mojentic; default builds are lean with no RUSTSEC advisories

## Branching Workflow

Trunk-based development: `main` is the only long-lived branch. All work lands on `main` via direct commit. Feature branches are not pushed to `origin`. Pull requests are not used. Short-lived local working branches (e.g. hopper worktrees) are merged to `main` and deleted locally before work is considered complete.

## CI / Release

Two GitHub Actions workflows: `ci.yml` (on push/PR) and `release.yml` (on
pushing a `v*` tag). `release.yml` runs the quality gate, builds binaries for
three targets (macOS arm64/x64, Linux x64), creates a GitHub Release with the
archives, and **overwrites the Homebrew tap formula** (`svetzal/homebrew-tap`,
`Formula/cmx.rb`) with the tag's version. There is no crates.io publish.

Homebrew is the **distribution** channel for end users. This **development
machine installs locally via `cargo install`**, not Homebrew — so after a
release, refresh the local install as an explicit extra step (see step 3).

Releasing is three distinct steps:

1. **Prep** (a normal commit on `main`): bump `version` in the root
   `Cargo.toml` (workspace version; `cmx`/`cmf` inherit it via
   `version.workspace = true`), run a build so `Cargo.lock` picks up the new
   `cmx`/`cmf` versions, finalize `CHANGELOG.md` (date the new section, open a
   fresh `## [Unreleased]`), and commit as `chore(release): prepare X.Y.Z`.
   This does **not** tag — nothing publishes yet.
2. **Tag** (the publish trigger): create a **lightweight** tag `vX.Y.Z` (match
   existing tag style — `git tag vX.Y.Z`, not `-a`) and push it. The push fires
   `release.yml`.
3. **Local install** (this machine): once the release is published, install the
   same version locally via `cargo install`. Both binaries are installed
   **lean** (default features, no `llm`), release profile, from the workspace
   member paths:

   ```bash
   cargo install --path cmx --force
   cargo install --path cmf --force
   ```

   Run from a checkout at the released version (the tagged commit, or `main` at
   the same `version`). `--force` overwrites the previously installed binaries.
   Verify with `cmx --version` / `cmf --version`.

Conventions and gotchas:

- **Sequence releases — never push two `v*` tags concurrently.** The Homebrew
  job overwrites the single tap formula with whichever run finishes last, so two
  in-flight releases can leave the tap pinned to the wrong (older) version. Push
  one tag, wait for its run to complete (`gh run watch <id> --exit-status`,
  including the Homebrew job), then push the next.
- The tag's commit must pass the quality gate independently (the workflow re-runs
  fmt/clippy/test/deny on the tagged tree).
- `--generate-notes` builds the GitHub Release body from commits; no manual
  release notes needed.
- Semver: new backward-compatible commands/features → minor bump; fixes → patch.

## Reference repositories

- **guidelines**: `~/Work/Projects/Personal/guidelines` — the reference source repository used for local testing of cmx features (artifact scanning, install, versioning, upgrades).
- **mojentic**: published crate (`mojentic` on crates.io), source at `~/Work/Projects/Personal/mojentic-unify/mojentic-ru` for reference. Used for LLM-powered analysis features (e.g. diff analysis between source and installed artifacts). Key usage patterns:
  - `OllamaGateway::new()` for local LLM access
  - `LlmBroker::new(model, gateway, None)` to create a broker
  - `broker.generate(&messages, None, None, None).await` for completions
  - `LlmMessage::system(...)` / `LlmMessage::user(...)` for message construction
  - Async-only (requires tokio runtime)

## Architecture

Entry points:
- `src/main.rs` — binary entry point; constructs AppContext with real gateways and dispatches CLI commands
- `src/lib.rs` — crate root; re-exports all public modules
- `src/cli.rs` — clap CLI definition
- `src/context.rs` — AppContext: bundles all I/O gateway dependencies for command invocations

Source management:
- `src/source.rs` — `cmx source` subcommands (add, list, browse, update, remove)
- `src/source_update.rs` — source update logic (git pull for registered sources)
- `src/source_iter.rs` — iterator over configured sources

Artifact scanning:
- `src/scan.rs` — artifact detection (walks source repos, matches agents/skills by frontmatter)
- `src/scan_marketplace.rs` — scans marketplace-structured plugin repos

Install/uninstall:
- `src/install.rs` — `cmx agent install` / `cmx skill install`
- `src/uninstall.rs` — `cmx agent uninstall` / `cmx skill uninstall`
- `src/copy.rs` — file copy helpers used by install

Query & display:
- `src/list.rs` — `cmx agent list` / `cmx skill list` / `cmx list`
- `src/outdated.rs` — `cmx outdated` (compare installed vs source)
- `src/search.rs` — `cmx search` (full-text search across sources)
- `src/info.rs` — `cmx info` (artifact detail view)
- `src/diff.rs` — LLM-powered diff analysis between installed and source versions (feature-gated)
- `src/display.rs` — output formatting for all commands
- `src/table.rs` — table rendering helpers

Config & persistence:
- `src/config.rs` — config dir paths, sources.json read/write
- `src/cmx_config.rs` — `cmx config` subcommands (show, set)
- `src/paths.rs` — ConfigPaths: global/local install dir resolution
- `src/lockfile.rs` — lock file read/write
- `src/json_file.rs` — generic JSON file load/save helpers
- `src/checksum.rs` — SHA-256 checksums for files and directories
- `src/fs_util.rs` — filesystem utility functions

Types:
- `src/types.rs` — shared types (SourceEntry, Artifact, ArtifactKind, LockFile, etc.)

Gateway (DI for testability):
- `src/gateway/mod.rs` — gateway module; re-exports traits and real implementations
- `src/gateway/filesystem.rs` — Filesystem trait for file I/O abstraction
- `src/gateway/git.rs` — GitClient trait for git operations
- `src/gateway/clock.rs` — Clock trait for time abstraction
- `src/gateway/llm.rs` — LlmClient trait for LLM access (feature-gated)
- `src/gateway/real.rs` — production implementations (RealFilesystem, RealGitClient, SystemClock, MojenticLlmClient)
- `src/gateway/fakes.rs` — in-memory fakes for tests (FakeFilesystem, FakeGitClient, etc.)

Test support:
- `src/test_support.rs` — test helpers shared across integration tests

## cmf — Context Mixer Forge

Publisher and authoring tool for managing agentic context artifacts.

### Architecture

- `cmf/src/main.rs` — binary entry point; dispatches CLI commands (including status)
- `cmf/src/lib.rs` — crate root; re-exports all public modules
- `cmf/src/cli.rs` — clap CLI definition (7 commands: facet, recipe, plugin, manifest, marketplace, validate, status)
- `cmf/src/repo.rs` — Repo root detection (marketplace, plugin, facets-only, unknown)
- `cmf/src/plugin.rs` — Plugin scanning, initialization, validation
- `cmf/src/plugin_types.rs` — Serde types for plugin.json and marketplace.json
- `cmf/src/marketplace.rs` — Marketplace validation and generation
- `cmf/src/facet.rs` — Facet scanning and validation
- `cmf/src/facet_types.rs` — Facet and Recipe structs, frontmatter parser
- `cmf/src/recipe.rs` — Recipe assembly and diffing
- `cmf/src/manifest.rs` — Multi-platform manifest generation
- `cmf/src/validate.rs` — Aggregate validation
- `cmf/src/display.rs` — formatting for plugin lists, recipes, facets, manifests, and validation results
- `cmf/src/validation.rs` — Shared validation types
- `cmf/src/test_support.rs` — test helpers for generating fake marketplace/plugin JSON

## Spec

See `SPEC.md` for the full design spec.
