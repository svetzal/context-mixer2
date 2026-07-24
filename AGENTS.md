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
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace && \
cargo deny check

# Full build (with LLM diff support)
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features
```

Note: `cmx-core` denies `missing_docs` at the crate level (see
`cmx-core/Cargo.toml`) and is fully documented; `cmx`/`cmf` currently only
warn on missing per-item docs (module-level `//!` headers are complete, but
not every `pub` item yet has a doc comment), so this workspace-wide
`-D warnings` doc command may still fail until that backlog is cleared —
track it as a follow-up, not a regression.

Additional recommended checks:

```bash
cargo audit          # security vulnerability scanning
cargo tarpaulin      # code coverage (target >80%)
```

- `--all-targets` is mandatory (ensures examples, tests, and benchmarks are checked)
- If any check fails, STOP immediately, fix the root cause (don't suppress), and re-run all checks
- Never use `#[allow(clippy::lint_name)]` without documenting why
- The `llm` feature gates tokio and mojentic; default builds are lean with no RUSTSEC advisories
- The architecture map is enforced by `cmx/tests/architecture_doc.rs` — adding, moving, or deleting a module requires updating the Architecture section in the same commit

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
     against the skill: `git log <last-vX.Y.Z-tag>..HEAD -- 'cmx/src/cli/**'
     'cmx/src/**'` shows what moved. For any command whose grammar, flags,
     defaults, deprecations, `--json` coverage, exit codes, or examples changed,
     rebuild and run `cmx --help` / `cmx <sub> --help`, and update the matching
     sections of `SKILL.md`. Do **not** hand-edit the skill's
     `metadata.version` — it is a `"0.0.0"` placeholder that `cmx init` stamps
     to the cmx binary version at install (`init::stamp_version`), so the skill
     version is locked to the workspace version automatically. **A release that
     changed the surface but not the skill's content is not ready to tag.**
   - **Reconcile the mdBook documentation.** The same `git log
     <last-vX.Y.Z-tag>..HEAD -- 'cmx/src/cli/**' 'cmx/src/**'` diff used for
     `SKILL.md` above must also be checked against `book/src/reference/commands.md`
     and the relevant `book/src/guide/*.md` pages. A new top-level command or
     `cmx set`-style subcommand family is covered by `cmx/tests/book_command_coverage.rs`,
     but flag/behavior changes to already-documented commands are not — update
     the matching table rows, grammar blocks, and worked examples by hand.
     **A release that changed the surface but not the book is not ready to tag.**
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

Each module's own `//!` header is the authoritative description of its purpose; the map below is an index, kept in sync with those headers and enforced by `cmx/tests/architecture_doc.rs`.

Entry points:

- `cmx/src/main.rs` — binary entry point; constructs AppContext with real gateways and dispatches CLI commands
- `cmx/src/lib.rs` — crate root; re-exports all public modules (including all `cmx-core` modules listed in the Re-exports section below)
- `cmx/src/cli/mod.rs` — clap CLI definition: imports, `COMPLETIONS_LONG_HELP`, `OutputArgs`, `Cli`, `Commands`; re-exports all action enums; submodules:
  `cmx/src/cli/source.rs`, `cmx/src/cli/set.rs`, `cmx/src/cli/artifact.rs`,
  `cmx/src/cli/home.rs`, `cmx/src/cli/config.rs`
- `cmx/src/dispatch/mod.rs` — command dispatch from `main.rs`; one submodule per command family:
  `cmx/src/dispatch/adopt.rs`, `cmx/src/dispatch/artifact.rs`, `cmx/src/dispatch/config.rs`,
  `cmx/src/dispatch/diff.rs`, `cmx/src/dispatch/info.rs`, `cmx/src/dispatch/set.rs`,
  `cmx/src/dispatch/source.rs`, `cmx/src/dispatch/test_support.rs`
- `cmx/src/completions.rs` — shell-completion generation
- `cmx/src/suggestions.rs` — suggestion helpers for commands
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
- `cmx/src/install/tests.rs` — install integration tests
- `cmx/src/uninstall.rs` — `cmx agent uninstall` / `cmx skill uninstall`
- `cmx/src/sync.rs` — `cmx skill sync`: reconcile a skill that diverged across platforms by copying one copy (newest version, or `--from <platform>`) over the others; works for external/source-less skills
- `cmx/src/sync/tests.rs` — sync integration tests
- `cmx/src/promote.rs` — `cmx skill promote` / `cmx agent promote`: the mirror of `install::update` — copy the in-place-edited installed copy back into the canonical home and refresh `home`-provenance lock baselines (home target only; git-sourced and reformatted-agent copies rejected)
- `cmx/src/promote/tests.rs` — promote integration tests
- `cmx/src/copy.rs` — file copy helpers used by install
- `cmx/src/platform_copies.rs` — shared primitive `gather_platform_copies` that iterates managed platforms filtered by `supports(kind)`, deduplicates by physical install path (collapsing e.g. Codex+Pi that share `.agents/skills`), and invokes a closure once per distinct copy; used by diff discovery, sync, and set deactivation

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
  `cmx/src/display/adopt.rs`, `cmx/src/display/config.rs`, `cmx/src/display/diff.rs`,
  `cmx/src/display/doctor/mod.rs`, `cmx/src/display/doctor/json.rs`,
  `cmx/src/display/info.rs`, `cmx/src/display/init.rs`,
  `cmx/src/display/install.rs`, `cmx/src/display/json.rs`, `cmx/src/display/list.rs`,
  `cmx/src/display/outdated.rs`, `cmx/src/display/promote.rs`, `cmx/src/display/search.rs`,
  `cmx/src/display/sets.rs`, `cmx/src/display/source.rs`, `cmx/src/display/sync.rs`,
  `cmx/src/display/uninstall.rs`, `cmx/src/display/util.rs`
- Tests for a `Display` impl live in the same `display/<command>.rs` module as the impl; core modules (`install.rs`, `uninstall.rs`, etc.) must not contain `Display`-formatting tests.
- `cmx/src/table.rs` — table rendering helpers

Sets:

- `cmx/src/sets/mod.rs` — `cmx set` subcommands (create, list, show, add, remove, activate, deactivate, delete, rename): locally-defined named groups of installed artifacts with a desired activation state (see `SETS.md`). `activate`/`deactivate` compose `install`/`uninstall` with reference-counting and a drift guard; `create --from-plugin <source>:<plugin>` seeds membership from a marketplace plugin's declared agents/skills (via `scan_marketplace::scan_marketplace_plugin`) without installing anything; `list`/`show` report context-footprint, and `doctor` checks set consistency
- `cmx/src/sets/types.rs` — set data types
- `cmx/src/sets/members.rs` — set membership management
- `cmx/src/sets/activation.rs` — set activation and deactivation logic

System survey / adoption:

- `cmx/src/doctor.rs` — `cmx doctor`: read-only system-wide survey of installed artifacts across platforms
- `cmx/src/doctor/survey.rs` — thin orchestrator: wires `locations`/`classify`/`aggregate`/`set_consistency` into the read-only `survey()` entry point
- `cmx/src/doctor/locations.rs` — resolves unique install locations across platforms/scopes and pre-loads lock files and source-provided artifact names
- `cmx/src/doctor/classify.rs` — classifies each installed artifact's state from its content checksum and assembles the raw per-location rows
- `cmx/src/doctor/aggregate.rs` — folds per-location rows into logical artifacts, consolidates state severity, sorts rows, and finds missing lock entries
- `cmx/src/doctor/divergence.rs` — detects divergence between installed artifacts and sources
- `cmx/src/doctor/set_consistency.rs` — set consistency checks used by `cmx doctor`
- `cmx/src/doctor/types.rs` — doctor result/report types
- `cmx/src/doctor/tests.rs` — doctor integration tests
- `cmx/src/adopt.rs` — `cmx adopt`: brings orphaned hand-authored artifacts under management
- `cmx/src/partition.rs` — batch classification of artifact names during adoption/partitioning

Config & persistence:

- `cmx/src/cmx_config.rs` — `cmx config` subcommands (show, set, external, platforms — the managed-platform allowlist that scopes install/uninstall/doctor)

Types:

- `cmx/src/error.rs` — `CliError` typed enum and `Result<T>` alias for all cmx command-core modules; transparent pass-through to `CmxError`; `Message(String)` escape hatch for runtime-built messages
- `cmx/src/flags.rs` — intent-revealing flag enums (`Force`, `RunMode`, `Purge`, `Selection`, `SurveyScope`) that replace positional `bool` parameters at internal call sites; each carries a `from_flag(bool)` constructor used exactly once at the clap dispatch boundary, and the enum itself (not a bool unwrapped from it) is threaded through core function signatures. `cmx/tests/flag_boundary.rs` is the architectural guard that enforces this — it fails the build if `force`/`purge`/`apply`/`local`/`include_local` reappears as a bare `bool` function parameter outside `cmx/src/cli/` or a report struct field
- `cmx/src/plugin_types.rs` — serde types for plugin.json and marketplace.json (single source of truth lifted from cmf)
- `cmx/src/codex_agent.rs` — transforms a cmx markdown agent into a Codex CLI subagent TOML document

Re-exports from cmx-core:

`cmx/src/lib.rs` re-exports the following modules from `cmx-core` so that `crate::` paths in cmx modules and tests resolve unchanged: `artifact_status`, `checksum`, `config`, `context`, `error_summary`, `fs_util`, `gateway`, `json_file`, `lockfile`, `paths`, `platform`, `platform_iter`, `targets`, `types`. Any edits to these modules belong in `cmx-core/src/`, not `cmx/src/`. Creating a file in `cmx/src/` with the same name as one of these re-exports (e.g. a new paths module) would shadow the re-export and is a mistake.

## cmx-core — Embeddable Core Library

The embeddable library; a path-dependency of `cmx`, published to crates.io on the `cmx-core-v*` tag channel, and twinned with the TypeScript port `cmx-core-ts` (npm). Both ports share a version number and must satisfy the same conformance suite.

**Behavioral contract:** `cmx-core/SPEC.md` and the shared golden fixtures under `cmx-core/conformance/` (categories: `checksum`, `frontmatter`, `version-guard`, `paths`, `target-resolve`, `install-e2e`) define byte-for-byte behavior every port must satisfy. The Rust port runs these via `cmx-core/src/conformance.rs` (`cargo test`); the TS port via `cmx-core-ts/test/conformance.test.ts` (`bun test`). A behavior change to one port is only complete when the shared SPEC/fixture is updated and both ports pass it.

### cmx-core Architecture

Types and platform:

- `cmx-core/src/lib.rs` — crate root; exports all public modules
- `cmx-core/src/types.rs` — shared types (SourceEntry, Artifact, ArtifactKind, LockFile, etc.)
- `cmx-core/src/platform.rs` — target AI-coding-assistant platform enum used for install-directory resolution
- `cmx-core/src/platform_iter.rs` — iterator over supported platforms
- `cmx-core/src/targets.rs` — target resolution helpers

Paths and persistence:

- `cmx-core/src/paths.rs` — ConfigPaths: global/local install dir resolution
- `cmx-core/src/checksum.rs` — SHA-256 checksums for files and directories
- `cmx-core/src/lockfile.rs` — lock file read/write
- `cmx-core/src/config/mod.rs` — config dir paths, sources.json read/write
- `cmx-core/src/config/installed.rs` — installed-artifact config records
- `cmx-core/src/json_file.rs` — generic JSON file load/save helpers
- `cmx-core/src/fs_util.rs` — filesystem utility functions

Skill lifecycle:

- `cmx-core/src/frontmatter.rs` — YAML frontmatter parsing and version stamping (the shared behavior the conformance suite gates)
- `cmx-core/src/skill_fs.rs` — skill filesystem helpers
- `cmx-core/src/skill_install/mod.rs` — `SkillInstaller` struct and `new()`; declares submodules; re-exports all public types
- `cmx-core/src/skill_install/plan.rs` — `plan()` method and planning helpers (`decide_action_for_entry`, `prepare_writes`, `build_lock_entry`)
- `cmx-core/src/skill_install/apply.rs` — `apply()`, `write_target_outcomes()`, `register_bundled_source()` methods
- `cmx-core/src/skill_install/status.rs` — `status()` method
- `cmx-core/src/skill_install/remove.rs` — `remove()` method
- `cmx-core/src/skill_install/types.rs` — skill install data types
- `cmx-core/src/skill_install/display.rs` — output formatting for skill install operations
- `cmx-core/src/skill_install/test_support.rs` — shared test helpers (`make_file`, `sample_skill`, `installer`, `plan_with_locked_version`); `#[cfg(test)]`-gated

Status and errors:

- `cmx-core/src/error.rs` — typed domain errors (`CmxError`, `LlmError`, `GitOp`, `Result<T>`) returned by all public cmx-core APIs; stable `.code()` discriminants mirror in the TypeScript port
- `cmx-core/src/artifact_status.rs` — artifact status determination: `source_outdated` (current/outdated/drifted logic) and `installed_is_newer` (semver guard for RefuseNewer behavior)
- `cmx-core/src/artifact_remove.rs` — shared primitive `remove_artifact_across_platforms` that collects distinct physical paths to delete (deduping shared dirs) and clears per-platform lock entries; used by both `cmx/src/uninstall.rs` and `cmx-core/src/skill_install/remove.rs`
- `cmx-core/src/error_summary.rs` — structured error summary types

Gateway (DI for testability):

- `cmx-core/src/context.rs` — AppContext: bundles all I/O gateway dependencies for command invocations
- `cmx-core/src/production.rs` — production AppContext construction with real gateways
- `cmx-core/src/gateway/mod.rs` — gateway module; re-exports traits and real implementations
- `cmx-core/src/gateway/filesystem.rs` — Filesystem trait for file I/O abstraction
- `cmx-core/src/gateway/git.rs` — GitClient trait for git operations
- `cmx-core/src/gateway/clock.rs` — Clock trait for time abstraction
- `cmx-core/src/gateway/llm.rs` — LlmClient trait for LLM access (feature-gated)
- `cmx-core/src/gateway/real.rs` — production implementations (RealFilesystem, RealGitClient, SystemClock, MojenticLlmClient)
- `cmx-core/src/gateway/fakes.rs` — in-memory fakes for tests (FakeFilesystem, FakeGitClient, etc.)

Test support and conformance:

- `cmx-core/src/test_support.rs` — test helpers shared across integration tests
- `cmx-core/src/conformance.rs` — conformance test runner (reads golden fixtures from `cmx-core/conformance/` and drives the Rust port)
- `cmx-core/src/bin/generate_conformance_fixtures.rs` — binary for regenerating conformance golden fixtures

## cmf — Context Mixer Forge

Publisher and authoring tool for managing agentic context artifacts.

### cmf Architecture

- `cmf/src/main.rs` — binary entry point; dispatches CLI commands (including status)
- `cmf/src/lib.rs` — crate root; re-exports all public modules
- `cmf/src/cli.rs` — clap CLI definition (7 commands: facet, recipe, plugin, manifest, marketplace, validate, status)
- `cmf/src/repo.rs` — Repo root detection (marketplace, plugin, facets-only, unknown)
- `cmf/src/plugin/mod.rs` — Plugin scanning, initialization, validation
- `cmf/src/plugin/validate.rs` — Plugin validation logic
- `cmf/src/plugin_types.rs` — thin re-export shim (`pub use cmx::plugin_types::{...}`); the serde types for plugin.json and marketplace.json now live in `cmx/src/plugin_types.rs` (single source of truth)
- `cmf/src/marketplace.rs` — Marketplace validation and generation
- `cmf/src/facet.rs` — Facet scanning and validation
- `cmf/src/facet_types.rs` — Facet and Recipe structs, frontmatter parser
- `cmf/src/recipe.rs` — Recipe assembly and diffing
- `cmf/src/manifest.rs` — Multi-platform manifest generation
- `cmf/src/validate.rs` — Aggregate validation
- `cmf/src/display/mod.rs` — formatting for plugin lists, recipes, facets, manifests, and validation results; submodules:
  `cmf/src/display/facet.rs`, `cmf/src/display/manifest.rs`, `cmf/src/display/plugin.rs`,
  `cmf/src/display/status.rs`, `cmf/src/display/validation.rs`
- `cmf/src/validation.rs` — Shared validation types
- `cmf/src/test_support.rs` — test helpers for generating fake marketplace/plugin JSON

## Spec

See `SPEC.md` for the full design spec.
