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
   same version locally via `cargo install`, release profile, from the workspace
   member paths:

   ```bash
   cargo install --path cmx --features llm --force
   cargo install --path cmf --force
   ```

   `cmx` is installed **with the `llm` feature** so the LLM-backed commands
   (`cmx skill info`'s summary, `cmx diff`) work locally; it pulls tokio +
   mojentic and needs the configured gateway's credentials (e.g.
   `OPENAI_API_KEY`) in the environment at runtime. `cmf` stays **lean**
   (default features, no `llm`). Homebrew distribution remains lean for both.

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
- `cmx/src/main.rs` — binary entry point; constructs AppContext with real gateways and dispatches CLI commands
- `cmx/src/lib.rs` — crate root; re-exports all public modules
- `cmx/src/cli.rs` — clap CLI definition
- `cmx/src/context.rs` — AppContext: bundles all I/O gateway dependencies for command invocations

Source management:
- `cmx/src/source/mod.rs` — `cmx source` subcommands (add, list, browse, update, remove)
- `cmx/src/source/browse.rs` — `cmx source browse` interactive browsing
- `cmx/src/source_update.rs` — source update logic (git pull for registered sources)
- `cmx/src/source_iter.rs` — iterator over configured sources

Artifact scanning:
- `cmx/src/scan/mod.rs` — artifact detection (walks source repos, matches agents/skills by frontmatter)
- `cmx/src/scan/frontmatter.rs` — YAML frontmatter parsing for artifact detection
- `cmx/src/scan_marketplace.rs` — scans marketplace-structured plugin repos

Install/uninstall:
- `cmx/src/install.rs` — `cmx agent install` / `cmx skill install`
- `cmx/src/uninstall.rs` — `cmx agent uninstall` / `cmx skill uninstall`
- `cmx/src/sync.rs` — `cmx skill sync`: reconcile a skill that diverged across platforms by copying one copy (newest version, or `--from <platform>`) over the others; works for external/source-less skills
- `cmx/src/copy.rs` — file copy helpers used by install

Query & display:
- `cmx/src/list.rs` — `cmx agent list` / `cmx skill list` / `cmx list`
- `cmx/src/outdated.rs` — `cmx outdated` (compare installed vs source)
- `cmx/src/search.rs` — `cmx search` (full-text search across sources)
- `cmx/src/info/mod.rs` — `cmx info` (artifact detail view)
- `cmx/src/info/summary.rs` — LLM-backed prose summary for `cmx info` (feature-gated)
- `cmx/src/diff.rs` — LLM-powered diff analysis between installed and source versions (feature-gated)
- `cmx/src/display/mod.rs` — output formatting for all commands; one submodule per command:
  `adopt.rs`, `config.rs`, `diff.rs`, `doctor.rs`, `info.rs`, `install.rs`, `list.rs`,
  `outdated.rs`, `search.rs`, `source.rs`, `uninstall.rs`
- Tests for a `Display` impl live in the same `display/<command>.rs` module as the impl; core modules (`install.rs`, `uninstall.rs`, etc.) must not contain `Display`-formatting tests.
- `cmx/src/table.rs` — table rendering helpers

System survey / adoption:
- `cmx/src/doctor.rs` — `cmx doctor`: read-only system-wide survey of installed artifacts across platforms
- `cmx/src/doctor/survey.rs` — walks platform install dirs and cross-references lock files
- `cmx/src/doctor/divergence.rs` — detects divergence between installed artifacts and sources
- `cmx/src/doctor/types.rs` — doctor result/report types
- `cmx/src/adopt.rs` — `cmx adopt`: brings orphaned hand-authored artifacts under management
- `cmx/src/partition.rs` — batch classification of artifact names during adoption/partitioning

Config & persistence:
- `cmx/src/config/mod.rs` — config dir paths, sources.json read/write
- `cmx/src/config/installed.rs` — installed-artifact config records
- `cmx/src/cmx_config.rs` — `cmx config` subcommands (show, set, external, platforms — the managed-platform allowlist that scopes install/uninstall/doctor)
- `cmx/src/paths.rs` — ConfigPaths: global/local install dir resolution
- `cmx/src/lockfile.rs` — lock file read/write
- `cmx/src/json_file.rs` — generic JSON file load/save helpers
- `cmx/src/checksum.rs` — SHA-256 checksums for files and directories
- `cmx/src/fs_util.rs` — filesystem utility functions

Types:
- `cmx/src/types.rs` — shared types (SourceEntry, Artifact, ArtifactKind, LockFile, etc.)
- `cmx/src/plugin_types.rs` — serde types for plugin.json and marketplace.json (single source of truth lifted from cmf)
- `cmx/src/platform.rs` — target AI-coding-assistant platform enum used for install-directory resolution
- `cmx/src/codex_agent.rs` — transforms a cmx markdown agent into a Codex CLI subagent TOML document

Gateway (DI for testability):
- `cmx/src/gateway/mod.rs` — gateway module; re-exports traits and real implementations
- `cmx/src/gateway/filesystem.rs` — Filesystem trait for file I/O abstraction
- `cmx/src/gateway/git.rs` — GitClient trait for git operations
- `cmx/src/gateway/clock.rs` — Clock trait for time abstraction
- `cmx/src/gateway/llm.rs` — LlmClient trait for LLM access (feature-gated)
- `cmx/src/gateway/real.rs` — production implementations (RealFilesystem, RealGitClient, SystemClock, MojenticLlmClient)
- `cmx/src/gateway/fakes.rs` — in-memory fakes for tests (FakeFilesystem, FakeGitClient, etc.)

Test support:
- `cmx/src/test_support.rs` — test helpers shared across integration tests

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
