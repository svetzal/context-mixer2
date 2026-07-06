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
`Formula/cmx.rb`) with the tag's version. `release.yml` itself publishes
neither to crates.io nor npm — those are the `cmx-core` library channels (see
"cmx-core & cmx-core-ts" below).

Homebrew is the **distribution** channel for end users. This **development
machine installs locally via `cargo install`**, not Homebrew — so after a
release, refresh the local install as an explicit extra step (see step 3).

Releasing is three distinct steps:

1. **Prep** (a normal commit on `main`). Do all of the following, then make one
   `chore(release): prepare X.Y.Z` commit — this does **not** tag, nothing
   publishes yet:
   - **Reconcile the embedded companion skill.** `cmx/skill/SKILL.md` is
     `include_str!`-embedded into the binary and installed to every user (and
     every driving agent) by `cmx init`, so a skill that lags the CLI ships
     wrong instructions to every consumer of the release. Diff the surface
     against the skill: `git log <last-vX.Y.Z-tag>..HEAD -- cmx/src/cli.rs
     'cmx/src/**'` shows what moved. For any command whose grammar, flags,
     defaults, deprecations, `--json` coverage, exit codes, or examples changed,
     rebuild and run `cmx --help` / `cmx <sub> --help`, and update the matching
     sections of `SKILL.md`. Do **not** hand-edit the skill's
     `metadata.version` — it is a `"0.0.0"` placeholder that `cmx init` stamps
     to the cmx binary version at install (`init::stamp_version`), so the skill
     version is locked to the workspace version automatically. **A release that
     changed the surface but not the skill's content is not ready to tag.**
   - Bump `version` in the root `Cargo.toml` (workspace version; `cmx`/`cmf`
     inherit it via `version.workspace = true`). `cmx-core` versions
     independently on its own tag channel — do not touch it in a cmx-only
     release. **If cmx-core's source has changed since its last `cmx-core-v*`
     tag, do not cut a cmx-only release at all** — use the coordinated
     cmx-core release process below so the crate, its TS twin, and the CLI move
     together.
   - Run a build so `Cargo.lock` picks up the new `cmx`/`cmf` versions.
   - Finalize `CHANGELOG.md` (date the new section, open a fresh
     `## [Unreleased]`).
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

### cmx-core & cmx-core-ts — one library, two ports, released in lockstep

`cmx-core` (Rust crate, published to **crates.io** via `cmx-core-v*` tags) and
`cmx-core-ts` (npm package `cmx-core`, published via `cmx-core-ts-v*` tags using
OIDC trusted publishing) are two ports of the **same** embeddable library. They
MUST stay behaviorally synchronized and share a version number.

**The conformance suite is the contract, not a suggestion.** `cmx-core/SPEC.md`
plus the shared golden fixtures under `cmx-core/conformance/` (checksum,
frontmatter, version-guard, paths, target-resolve, install-e2e) define the
byte-for-byte behavior every port must satisfy. Both ports run these same
fixtures — the Rust crate via `cargo test` (`conformance.rs`), the TS port via
`bun test` (`cmx-core-ts/test/conformance.test.ts`, which reads
`../../cmx-core/conformance`). A behavior change to one port is only "done" when
(a) the shared SPEC/fixture is updated to encode the new behavior, and (b) BOTH
ports pass it. Never change behavior in one port without landing the fixture and
making the other port green — a port that lags the fixtures is a release blocker.

**Version lockstep.** The crates.io crate and the npm package carry the same
version; bump and release them together, even when only one port's source
changed — the matching version on the unchanged port asserts continued
conformance parity (its passing `cargo test` / `bun test` against the current
fixtures is the proof). Do not let the two ports drift to different versions.

**Coordinated release (whenever cmx-core has changed).** cmx depends on cmx-core
by path, so a cmx binary always compiles the current core source; but the
library's own consumers (crates.io, npm — e.g. hopper) only receive changes when
the library is published. When cmx-core's source has changed, release all three
artifacts in one coordinated pass instead of a cmx-only `v*` release:

1. **Prep (one commit on `main`).**
   - Bump `cmx-core/Cargo.toml` and `cmx-core-ts/package.json` to the **same**
     new version (semver on the library's observable behavior: new behavior →
     minor, fixes → patch).
   - Add a dated entry to `cmx-core/CHANGELOG.md` (SPEC/fixture deltas, behavior
     fixes, refactors).
   - Bump the workspace `version` (`cmx`/`cmf`) per its own semver and add the
     matching `CHANGELOG.md` entry — a bare "picks up cmx-core X.Y.Z" is a valid
     patch when cmx has no other change. Reconcile `cmx/skill/SKILL.md` if the
     CLI surface moved (see the cmx release steps above).
   - `cargo build` to refresh `Cargo.lock`.
   - Run BOTH gate suites green before committing: the cmx quality gate above,
     AND `cd cmx-core-ts && bun run typecheck && bun run lint && bun test`.
   - Commit `chore(release): prepare cmx X.Y.Z + cmx-core A.B.C`.
2. **Tag & publish one channel at a time — never fire two publishing tags
   concurrently.** Push each tag, watch its workflow to green
   (`gh run watch <id> --exit-status`) before pushing the next:
   - `cmx-core-vA.B.C` → `publish-cmx-core.yml` → **crates.io** (the job guards
     that the tag matches `cmx-core/Cargo.toml`'s version).
   - `cmx-core-ts-vA.B.C` → `publish-cmx-core-ts.yml` → **npm** (OIDC).
   - `vX.Y.Z` → `release.yml` → cmx/cmf binaries + Homebrew.

   crates.io and npm publishes cannot be overwritten (only yanked) — treat these
   two pushes as irreversible and confirm the prep is right before pushing.
3. **Local install** the new cmx/cmf exactly as in the cmx release steps above,
   and verify.

If cmx-core has **not** changed since its last `cmx-core-v*` tag, skip all of
this and cut a normal cmx-only `v*` release.

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
- `cmx/src/init.rs` — `cmx init`: install/remove cmx's own companion skill (embedded `skill/SKILL.md`) via `cmx-core`'s `SkillInstaller`, following the `<tool> init` convention

Source management:

- `cmx/src/source/mod.rs` — `cmx source` subcommands (add, list, browse, update, remove)
- `cmx/src/source/browse.rs` — `cmx source browse` interactive browsing
- `cmx/src/source_update.rs` — source update logic (git pull for registered sources)
- `cmx/src/source_iter.rs` — iterator over configured sources

Artifact scanning:

- `cmx/src/scan/mod.rs` — artifact detection (walks source repos, matches agents/skills by frontmatter)
- `cmx/src/scan/frontmatter.rs` — YAML frontmatter parsing for artifact detection
- `cmx/src/scan/yaml_repair.rs` — frontmatter normalization (tabs→spaces, quoting stray `>`/`|` values) applied before YAML parsing to tolerate real-world non-spec artifacts
- `cmx/src/scan_marketplace.rs` — scans marketplace-structured plugin repos

Install/uninstall:

- `cmx/src/install.rs` — `cmx agent install` / `cmx skill install`
- `cmx/src/uninstall.rs` — `cmx agent uninstall` / `cmx skill uninstall`
- `cmx/src/sync.rs` — `cmx skill sync`: reconcile a skill that diverged across platforms by copying one copy (newest version, or `--from <platform>`) over the others; works for external/source-less skills
- `cmx/src/promote.rs` — `cmx skill promote` / `cmx agent promote`: the mirror of `install::update` — copy the in-place-edited installed copy back into the canonical home and refresh `home`-provenance lock baselines (home target only; git-sourced and reformatted-agent copies rejected)
- `cmx/src/copy.rs` — file copy helpers used by install

Query & display:

- `cmx/src/list.rs` — `cmx agent list` / `cmx skill list` / `cmx list`
- `cmx/src/outdated.rs` — `cmx outdated` (compare installed vs source)
- `cmx/src/search.rs` — `cmx search` (full-text search across sources)
- `cmx/src/info/mod.rs` — `cmx info` (artifact detail view)
- `cmx/src/info/summary.rs` — LLM-backed prose summary for `cmx info` (feature-gated)
- `cmx/src/diff/mod.rs` — `cmx diff` orchestration: entry point, gather loop, source lookup, copy-focus selection; public result types (`DiffOutput`, `CopyStatus`, `FileStatus`, `FileChange`, `Reconciliation`, `FocusedComparison`)
- `cmx/src/diff/discovery.rs` — installed-copy discovery: `InstalledCopy`, `CopyEval`, `discover_copies`, `gather_skill_copies`, `evaluate_copies`, `representative_platform`
- `cmx/src/diff/structural.rs` — per-file structural diff: `ArtifactDiff`, `diff_artifact`, `diff_files`, `diff_dirs`, `modified_file_block`, `collect_relative_files_with`
- `cmx/src/diff/reconcile.rs` — lock-state reconciliation: `focus_lock_state`, `reconciliations`
- `cmx/src/diff/analyze.rs` — LLM-powered analysis (feature-gated path): `analyze_focus`
- `cmx/src/text_diff.rs` — general line-oriented LCS text differ (`split_lines`/`lcs_ops`/`render_hunks`); pure, no coupling to the artifact model
- `cmx/src/display/mod.rs` — output formatting for all commands; one submodule per command:
  `adopt.rs`, `config.rs`, `diff.rs`, `doctor.rs`, `info.rs`, `init.rs`, `install.rs`, `list.rs`,
  `outdated.rs`, `promote.rs`, `search.rs`, `sets.rs`, `source.rs`, `sync.rs`, `uninstall.rs`, `util.rs`
- Tests for a `Display` impl live in the same `display/<command>.rs` module as the impl; core modules (`install.rs`, `uninstall.rs`, etc.) must not contain `Display`-formatting tests.
- `cmx/src/table.rs` — table rendering helpers

Sets:

- `cmx/src/sets.rs` — `cmx set` subcommands (create, list, show, add, remove, activate, deactivate, delete, rename): locally-defined named groups of installed artifacts with a desired activation state (see `SETS.md`). `activate`/`deactivate` compose `install`/`uninstall` with reference-counting and a drift guard; `create --from <source>:<plugin>` seeds membership from a marketplace plugin's declared agents/skills (via `scan_marketplace::scan_marketplace_plugin`) without installing anything; `list`/`show` report context-footprint, and `doctor` checks set consistency

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

### cmf Architecture

- `cmf/src/main.rs` — binary entry point; dispatches CLI commands (including status)
- `cmf/src/lib.rs` — crate root; re-exports all public modules
- `cmf/src/cli.rs` — clap CLI definition (7 commands: facet, recipe, plugin, manifest, marketplace, validate, status)
- `cmf/src/repo.rs` — Repo root detection (marketplace, plugin, facets-only, unknown)
- `cmf/src/plugin.rs` — Plugin scanning, initialization, validation
- `cmf/src/plugin_types.rs` — thin re-export shim (`pub use cmx::plugin_types::{...}`); the serde types for plugin.json and marketplace.json now live in `cmx/src/plugin_types.rs` (single source of truth)
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
