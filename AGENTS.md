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

## Release Process

To create a new release:

1. **Pre-flight** — all quality gates pass (see above)
2. **Update CHANGELOG.md** — move `[Unreleased]` entries to `[X.Y.Z]` with today's date
3. **Bump version** in `Cargo.toml` workspace (`[workspace.package] version`)
4. **Commit**: `Release vX.Y.Z`
5. **Tag**: `git tag vX.Y.Z`
6. **Push**: `git push origin main --tags`
7. **CI** (`.github/workflows/release.yml`) triggers on the `v*` tag:
   - Runs full quality gate (fmt, clippy, test, deny — default and all-features)
   - Builds cross-platform binaries (macOS ARM64, macOS x64, Linux x64) with `--features llm`
   - Creates GitHub Release with archives containing both `cmx` and `cmf`
   - Updates Homebrew tap (`svetzal/tap`) with new formula and checksums
8. **Local install** (immediately, don't wait for Homebrew):
   ```bash
   cargo install --path cmx && cargo install --path cmf
   ```

## Reference repositories

- **guidelines**: `~/Work/Projects/Personal/guidelines` — the reference source repository used for local testing of cmx features (artifact scanning, install, versioning, upgrades).
- **mojentic**: published crate (`mojentic` on crates.io), source at `~/Work/Projects/Personal/mojentic-unify/mojentic-ru` for reference. Used for LLM-powered analysis features (e.g. diff analysis between source and installed artifacts). Key usage patterns:
  - `OllamaGateway::new()` for local LLM access
  - `LlmBroker::new(model, gateway, None)` to create a broker
  - `broker.generate(&messages, None, None, None).await` for completions
  - `LlmMessage::system(...)` / `LlmMessage::user(...)` for message construction
  - Async-only (requires tokio runtime)

## Architecture

- `src/cli.rs` — clap CLI definition
- `src/config.rs` — config dir paths, sources.json read/write
- `src/source.rs` — `cmx source` subcommands (add, list, browse, update, remove)
- `src/install.rs` — `cmx agent install` / `cmx skill install`
- `src/list.rs` — `cmx agent list` / `cmx skill list` / `cmx list`
- `src/outdated.rs` — `cmx outdated` (compare installed vs source)
- `src/scan.rs` — artifact detection (walks source repos, matches agents/skills by frontmatter)
- `src/checksum.rs` — SHA-256 checksums for files and directories
- `src/lockfile.rs` — lock file read/write
- `src/types.rs` — shared types (SourceEntry, Artifact, ArtifactKind, LockFile, etc.)

## cmf — Context Mixer Forge

Publisher and authoring tool for managing agentic context artifacts.

### Architecture

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
- `cmf/src/status.rs` — Repo overview dashboard
- `cmf/src/validation.rs` — Shared validation types

## Spec

See `SPEC.md` for the full design spec.
