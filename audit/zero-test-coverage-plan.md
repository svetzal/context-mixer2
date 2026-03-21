Now I have a thorough understanding of the entire codebase. Here is the complete plan:

---

# Plan: Add Test Coverage to cmx

## Overview

The cmx project has zero tests across 16 source files. This plan addresses the issue in incremental, safe steps — starting with structural changes that enable testing, then adding unit tests for pure functions (immediate wins), then serde round-trip tests, and finally introducing gateway traits for testing I/O-coupled logic. Each step is independently committable and moves the codebase toward an executable specification of correctness.

**Invariants preserved**: Rust edition 2024, toolchain 1.94.0, tokio runtime, no MSRV change, no new public API surface, no dependency changes except dev-dependencies for testing.

---

## Step 1: Create `lib.rs` and Re-export Modules

**Why**: The project is binary-only (`main.rs` declares all modules as `mod`). Without a `lib.rs`, integration tests in `tests/` cannot access any module, and doctests have no public items to attach to. This structural change gates everything else.

**What to do**:

1. Create `src/lib.rs` with public module declarations for all modules that need testing:
   ```rust
   pub mod checksum;
   pub mod cli;
   pub mod cmx_config;
   pub mod config;
   pub mod diff;
   pub mod info;
   pub mod install;
   pub mod list;
   pub mod lockfile;
   pub mod outdated;
   pub mod scan;
   pub mod search;
   pub mod source;
   pub mod types;
   pub mod uninstall;
   ```

2. Update `src/main.rs` to use the library crate instead of declaring modules directly:
   ```rust
   use cmx::cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
   use cmx::types::ArtifactKind;
   // ... rest of main uses cmx:: prefix for all module references
   ```
   Remove all `mod` declarations from `main.rs`.

3. Update `Cargo.toml` — no changes needed; Cargo auto-detects `lib.rs` alongside `main.rs` in the same package. However, verify the binary builds correctly by running `cargo build`.

4. Make functions/types that need to be tested `pub` or `pub(crate)` as appropriate. Currently most module-level functions are already `pub`. The private helper functions (like `extract_field`, `parse_name`, `truncate_description`, `looks_like_url`, `is_stale`, `count_artifacts`, `format_deprecation`, `status_indicator`, `parse_deprecation`) need to become either:
   - `pub(crate)` if they should be tested via `mod tests` blocks within each module (preferred for internal helpers), OR
   - Remain private and tested within inline `#[cfg(test)] mod tests` blocks in the same file (no visibility change needed since tests within the same file can access private items)

   **Decision**: Use inline `#[cfg(test)] mod tests` blocks for private function tests. This is the idiomatic Rust approach — no visibility changes needed for Step 2.

5. Run quality gates: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo deny check`

**Commit**: "Add lib.rs to enable integration tests and doctests"

---

## Step 2: Add Unit Tests for Pure Functions in `scan.rs`

**Why**: `scan.rs` contains the frontmatter parser, which is the core artifact detection mechanism. It has three easily testable pure functions: `extract_field()`, `parse_deprecation()`, and the frontmatter parsing logic (once separated from I/O). These are high-value because bugs here cause artifacts to be silently missed or misidentified.

**What to do**:

1. Refactor `parse_frontmatter()` and `parse_agent_frontmatter()` to separate I/O from logic:
   - Extract a new function `parse_frontmatter_str(content: &str) -> Option<Frontmatter>` that contains the pure parsing logic currently inside `parse_frontmatter()` (lines 169–179, everything after `fs::read_to_string`).
   - Extract a new function `parse_agent_frontmatter_str(content: &str) -> Option<Frontmatter>` that contains the pure parsing logic currently inside `parse_agent_frontmatter()` (lines 183–201, everything after `fs::read_to_string`).
   - Have the original `parse_frontmatter()` and `parse_agent_frontmatter()` call `fs::read_to_string` then delegate to the `_str` variants.
   - Make `Frontmatter` struct `pub(crate)` so tests can inspect its fields (or just test via the returned `Option`).

2. Add `#[cfg(test)] mod tests` block to `scan.rs` with the following test cases:

   **`extract_field` tests**:
   - Basic field extraction: `"name: my-agent\ndescription: A thing"` → `extract_field(text, "name")` returns `Some("my-agent")`
   - Field with quoted value: `"name: \"my-agent\""` → returns `Some("my-agent")` (quotes stripped)
   - Field not present → returns `None`
   - Empty field value: `"name: "` → returns `None` (filtered by `.filter(|v| !v.is_empty())`)
   - Field with extra whitespace: `"name:   spaced  "` → returns `Some("spaced")` (trimmed)
   - Multiple fields, get specific one
   - Field prefix collision: key `"name"` should not match line `"namespace: foo"`

   **`parse_deprecation` tests**:
   - `deprecated: true` with reason and replacement → returns `Some(Deprecation { reason: Some(...), replacement: Some(...) })`
   - `deprecated: true` with no reason/replacement → returns `Some(Deprecation { reason: None, replacement: None })`
   - `deprecated: false` → returns `None`
   - No deprecated field → returns `None`

   **`parse_frontmatter_str` tests**:
   - Valid frontmatter with all fields → returns `Some(Frontmatter)` with correct values
   - No frontmatter delimiters → returns `None`
   - Missing closing `---` → returns `None`
   - Frontmatter with version → version is `Some`
   - Frontmatter without version → version is `None`
   - Frontmatter with deprecation fields → deprecation is populated

   **`parse_agent_frontmatter_str` tests**:
   - Valid agent frontmatter (has `name:` and `description:` lines) → returns `Some`
   - Missing `name:` field → returns `None` (agent requires both name and description)
   - Missing `description:` field → returns `None`
   - Has both fields → returns `Some` with correct description and version

3. Run quality gates.

**Commit**: "Add unit tests for frontmatter parsing in scan.rs"

---

## Step 3: Add Unit Tests for Pure Functions in `install.rs`

**Why**: `parse_name()` handles the `source:artifact` name resolution syntax. Getting this wrong silently installs the wrong artifact or fails with confusing errors.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `install.rs` with tests for `parse_name()`:

   - `"guidelines:rust-craftsperson"` → `(Some("guidelines"), "rust-craftsperson")`
   - `"rust-craftsperson"` → `(None, "rust-craftsperson")`
   - `"a:b:c"` → `(Some("a"), "b:c")` (split_once behavior — only splits on first colon)
   - `":name"` → `(Some(""), "name")` (edge case: empty source)
   - `"source:"` → `(Some("source"), "")` (edge case: empty artifact name)

2. Run quality gates.

**Commit**: "Add unit tests for artifact name parsing in install.rs"

---

## Step 4: Add Unit Tests for Pure Functions in `source.rs`

**Why**: `looks_like_url()`, `is_stale()`, `count_artifacts()`, and `format_deprecation()` are all pure functions with high impact on user-facing behavior. `is_stale()` controls auto-update timing, `looks_like_url()` determines whether a source is treated as local or git.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `source.rs` with tests:

   **`looks_like_url` tests**:
   - `"https://github.com/foo/bar"` → `true`
   - `"http://example.com"` → `true`
   - `"git@github.com:foo/bar.git"` → `true`
   - `"ssh://git@github.com/foo/bar"` → `true`
   - `"/home/user/repos/guidelines"` → `false`
   - `"./relative/path"` → `false`
   - `"just-a-name"` → `false`
   - `""` (empty) → `false`

   **`is_stale` tests**:
   - Entry with `last_updated: None` → `true` (never updated)
   - Entry with `last_updated` set to an unparseable string → `true`
   - Entry with `last_updated` 120 minutes ago → `true`
   - Entry with `last_updated` 30 minutes ago → `false`
   - Entry with `last_updated` exactly at the threshold (60 min) → `true` (uses `>=`)

   For `is_stale` tests, construct `SourceEntry` values directly — the struct fields are all public and serializable.

   **`count_artifacts` tests**:
   - Empty list → `(0, 0)`
   - Mixed agents and skills → correct counts
   - All agents → `(n, 0)`
   - All skills → `(0, n)`

   For `count_artifacts` tests, construct `Artifact` enum variants directly.

   **`format_deprecation` tests**:
   - Non-deprecated artifact → empty string
   - Deprecated with reason and replacement → `"  ⛔ DEPRECATED: reason (use replacement instead)"`
   - Deprecated with reason only → `"  ⛔ DEPRECATED: reason"`
   - Deprecated with replacement only → `"  ⛔ DEPRECATED (use replacement instead)"`
   - Deprecated with neither → `"  ⛔ DEPRECATED"`

2. Run quality gates.

**Commit**: "Add unit tests for URL detection, staleness, and formatting in source.rs"

---

## Step 5: Add Unit Tests for Pure Functions in `list.rs` and `search.rs`

**Why**: `status_indicator()` drives the user-facing status column in `cmx list`, and `truncate_description()` controls search result display. Both are pure and trivially testable.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `list.rs` with tests for `status_indicator()`:

   - `("-", "-", false)` → `" "` (unmanaged, no source)
   - `("-", "1.0.0", false)` → `"⚠️"` (installed but unversioned)
   - `("1.0.0", "-", false)` → `" "` (no source version)
   - `("1.0.0", "1.0.0", false)` → `"✅"` (up to date)
   - `("1.0.0", "2.0.0", false)` → `"⚠️"` (behind)
   - `("1.0.0", "1.0.0", true)` → `"⛔"` (deprecated overrides everything)
   - `("-", "-", true)` → `"⛔"` (deprecated overrides)

2. Add `#[cfg(test)] mod tests` block to `search.rs` with tests for `truncate_description()`:

   - Short string (under max_len) → returned as-is
   - String exactly at max_len → returned as-is
   - Long string → truncated with `"..."` suffix
   - String with `\n` (literal backslash-n) → takes first part before `\n`
   - String with actual newline → takes first line
   - Empty string → returns empty
   - String with leading/trailing whitespace → trimmed

3. Run quality gates.

**Commit**: "Add unit tests for status indicators and description truncation"

---

## Step 6: Add Unit Tests for `config.rs` Pure Functions

**Why**: `resolve_local_path()` and `install_dir()` are pure path construction functions that determine where artifacts get installed. Mistakes here cause installs to the wrong directory.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `config.rs`:

   **`resolve_local_path` tests**:
   - Local source entry with `path: Some(PathBuf::from("/foo/bar"))` → returns `/foo/bar`
   - Git source entry with `local_clone: Some(PathBuf::from("/tmp/clone"))` → returns `/tmp/clone`
   - Local source with `path: None` → returns empty PathBuf (current behavior, may want to document)
   - Git source with `local_clone: None` → returns empty PathBuf

   **`install_dir` tests** (note: `install_dir` calls `dirs::home_dir()` for global scope, so test only the local scope variant which is pure):
   - `install_dir(ArtifactKind::Agent, true)` → `.claude/agents`
   - `install_dir(ArtifactKind::Skill, true)` → `.claude/skills`

2. Run quality gates.

**Commit**: "Add unit tests for path resolution in config.rs"

---

## Step 7: Add Serde Round-Trip Tests for Core Types

**Why**: `LockFile`, `SourcesFile`, `CmxConfig`, `LockEntry`, and `SourceEntry` are serialized to JSON on disk. If serde attributes (`rename`, `skip_serializing_if`, `rename_all`) break, lock files become unreadable or silently lose data. Round-trip tests catch this.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `types.rs` with round-trip tests:

   **`LockFile` round-trip**:
   - Construct a `LockFile` with 2 packages (one agent, one skill), serialize with `serde_json::to_string_pretty`, deserialize back, assert all fields match.
   - Specifically verify `artifact_type` serializes as `"agent"` / `"skill"` (lowercase via `rename_all`).
   - Verify `version: None` is omitted from JSON output (via `skip_serializing_if`).

   **`SourcesFile` round-trip**:
   - Construct with local and git source entries, round-trip, verify `type` field uses `"local"` / `"git"` (via `rename_all`).
   - Verify optional fields (`path`, `url`, `local_clone`, `branch`, `last_updated`) are omitted when `None`.

   **`CmxConfig` round-trip**:
   - Default config → serialize → deserialize → matches default.
   - Config with Ollama gateway → round-trip preserves gateway type.

   **`LlmGatewayType` Display**:
   - `LlmGatewayType::OpenAI.to_string()` → `"openai"`
   - `LlmGatewayType::Ollama.to_string()` → `"ollama"`

   **`ArtifactKind` Display**:
   - `ArtifactKind::Agent.to_string()` → `"agent"`
   - `ArtifactKind::Skill.to_string()` → `"skill"`

   **`Artifact` accessor tests**:
   - Construct `Artifact::Agent { ... }` → verify `.name()`, `.description()`, `.kind()`, `.artifact_kind()`, `.path()`, `.version()`, `.deprecation()`, `.is_deprecated()` all return expected values.
   - Same for `Artifact::Skill { ... }`.

   **Golden file test** (optional but high-value):
   - Hardcode a JSON string representing a known-good lock file format.
   - Deserialize it and verify all fields parse correctly.
   - This catches regressions where serde attribute changes silently break compatibility with existing on-disk lock files.

2. Run quality gates.

**Commit**: "Add serde round-trip and accessor tests for core types"

---

## Step 8: Add Unit Tests for `lockfile.rs` Path Construction

**Why**: `lock_path()` determines where the lock file lives. Local scope uses `.context-mixer/cmx-lock.json` (relative), global scope uses `~/.config/context-mixer/cmx-lock.json`.

**What to do**:

1. Add `#[cfg(test)] mod tests` block to `lockfile.rs`:

   **`lock_path` tests**:
   - `lock_path(true)` → ends with `.context-mixer/cmx-lock.json` (relative path for local)
   - `lock_path(false)` → ends with `context-mixer/cmx-lock.json` (inside config dir)

2. Run quality gates.

**Commit**: "Add unit tests for lock file path construction"

---

## Step 9: Add Outdated Determination Logic Tests

**Why**: The `is_outdated` decision in `outdated.rs` (lines 127–143) and the status string construction (lines 149–155) are pure logic operating on `Option<&LockEntry>` and `SourceArtifactInfo`. They determine whether `cmx outdated` tells the user to update — false positives waste time, false negatives miss real changes.

**What to do**:

1. Extract the outdated determination logic into a testable function. Currently this logic lives inside `collect_outdated_for_scope()` interleaved with I/O. Extract it:

   ```rust
   /// Determine whether an artifact is outdated given its lock entry and source info.
   /// Returns None if not outdated, or Some(status_string) if it is.
   fn determine_outdated_status(
       lock_entry: Option<&LockEntry>,
       source_checksum: &str,
       source_version: Option<&str>,
   ) -> Option<String> {
       // ... extracted logic from lines 127-155
   }
   ```

2. Add `#[cfg(test)] mod tests` block to `outdated.rs` with tests:

   - Lock entry with matching checksum and version → `None` (not outdated)
   - Lock entry with different checksum → `Some("changed")`
   - Lock entry with matching checksum but source gained a version → `Some("untracked")`
   - No lock entry at all → `Some("untracked")`
   - Lock entry with different version strings and different checksums → `Some("update")`
   - Lock entry with no version, source has version, different checksum → `Some("untracked")`

3. Run quality gates.

**Commit**: "Extract and test outdated determination logic"

---

## Step 10: Deduplicate `collect_files()` Between `checksum.rs` and `diff.rs`

**Why**: `checksum.rs::collect_files()` and `diff.rs::collect_files_recursive()` both do recursive filesystem walks with nearly identical logic. This is knowledge duplication — if the dotfile-skipping logic in `checksum.rs` is changed, `diff.rs` won't follow. The assessment explicitly flags this.

**What to do**:

1. Create a shared utility — either in a new `src/fs_util.rs` module or in an existing module. Recommended: add `src/fs_util.rs`.

   ```rust
   // src/fs_util.rs
   use anyhow::Result;
   use std::path::{Path, PathBuf};
   use std::fs;

   /// Recursively collect all non-hidden files under `dir`, returning absolute paths.
   pub fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
       let mut files = Vec::new();
       collect_files_inner(dir, &mut files)?;
       Ok(files)
   }

   fn collect_files_inner(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
       for entry in fs::read_dir(dir)? {
           let entry = entry?;
           let path = entry.path();
           if let Some(name) = path.file_name() {
               if name.to_string_lossy().starts_with('.') {
                   continue;
               }
           }
           if path.is_dir() {
               collect_files_inner(&path, files)?;
           } else {
               files.push(path);
           }
       }
       Ok(())
   }
   ```

2. Update `checksum.rs` to use `crate::fs_util::collect_files()` instead of its private `collect_files()`.

3. Update `diff.rs::collect_relative_files()` to use `crate::fs_util::collect_files()` and then strip prefixes, instead of its own `collect_files_recursive()`. Note: `diff.rs::collect_files_recursive()` does NOT skip dotfiles while `checksum.rs::collect_files()` does — this is likely a bug in `diff.rs`. Verify with the user whether dotfiles should be skipped in diffs too. If so, the shared function fixes both. If not, add a parameter to control skipping.

4. Add the module to `lib.rs`: `pub mod fs_util;`

5. Run quality gates.

**Commit**: "Deduplicate recursive file collection into fs_util module"

---

## Step 11: Add Integration Tests Using Temp Directories

**Why**: Steps 2–9 cover pure function unit tests, but the critical install/uninstall/scan workflows involve filesystem operations that can only be validated end-to-end. Using temp directories provides isolated, reproducible integration tests.

**What to do**:

1. Create `tests/` directory.

2. Add `tempfile` as a dev-dependency in `Cargo.toml`:
   ```toml
   [dev-dependencies]
   tempfile = "3"
   ```

3. Create `tests/scan_integration.rs` with tests:

   **Marketplace scan**:
   - Create a temp dir with `.claude-plugin/marketplace.json` and sample agent/skill files with valid frontmatter.
   - Call `cmx::scan::scan_source()` on it.
   - Assert correct number and types of artifacts found.
   - Assert artifact names, descriptions, versions match frontmatter.

   **Walk-based scan (no marketplace)**:
   - Create a temp dir with `.md` files (agents) and directories with `SKILL.md` (skills).
   - Call `cmx::scan::scan_source()`.
   - Assert agents and skills are found.
   - Assert hidden directories (`.hidden/`) are skipped.
   - Assert `SKILL.md` itself is not treated as an agent.

   **Edge cases**:
   - Empty directory → returns empty vec.
   - Files without frontmatter → skipped.
   - Agent file without `name:` or `description:` in frontmatter → skipped (agent frontmatter requires both).

4. Create `tests/checksum_integration.rs` with tests:

   **File checksum**:
   - Write a known file to temp dir, compute checksum, verify it starts with `"sha256:"`.
   - Write the same content to another file, verify checksums match.
   - Write different content, verify checksums differ.

   **Directory checksum determinism**:
   - Create a temp dir with multiple files, compute checksum.
   - Recompute → same result (deterministic).
   - Change one file → checksum changes.
   - Verify dotfiles are excluded (create `.DS_Store`, checksum shouldn't change).

5. Run quality gates.

**Commit**: "Add integration tests for scanning and checksum computation"

---

## Step 12: Add Lock File Integration Tests

**Why**: Lock file corruption or format drift is the highest-risk failure mode — it means cmx loses track of what's installed. Testing the full load/save cycle with real files catches encoding issues.

**What to do**:

1. Create `tests/lockfile_integration.rs`:

   **Round-trip persistence**:
   - Note: `lockfile::load()` and `lockfile::save()` use `config::config_dir()` / relative paths, making them hard to test with temp dirs directly. Two options:
     a. Refactor `lockfile::load/save` to accept a `Path` parameter (preferred — makes them more testable and follows functional core principle).
     b. Use environment variable overrides (fragile, not recommended).

   **Recommended refactor**: Add `load_from(path: &Path)` and `save_to(lock: &LockFile, path: &Path)` functions that contain the actual logic. Have `load()` and `save()` compute the path and delegate. Then test the `_from`/`_to` variants.

   **Tests**:
   - Save a LockFile to temp path, load it back, verify all fields match.
   - Load from non-existent path → returns default (empty) LockFile.
   - Load from invalid JSON → returns error.
   - Save to path where parent dir doesn't exist → creates parent dirs.

2. Run quality gates.

**Commit**: "Refactor lockfile for testability and add integration tests"

---

## Step 13: Measure Coverage and Identify Remaining Gaps

**Why**: After steps 2–12, there should be meaningful coverage of the pure logic and critical integration paths. Measuring coverage identifies what's still untested and guides future work.

**What to do**:

1. Run `cargo tarpaulin` (already configured in `tarpaulin.toml`) and review the report.

2. Document any critical paths that remain untested and would need gateway traits (Phase 2 from the assessment) to test properly:
   - `install()` end-to-end workflow
   - `uninstall()` end-to-end workflow
   - Git operations in `source.rs`
   - LLM-powered diff analysis in `diff.rs`

3. If coverage is below 50%, prioritize adding more integration tests for `install`/`uninstall` using temp directories (these can be done without gateway traits by setting up real temp directory structures).

4. Run full quality gates one final time.

**Commit**: No code change — this is an assessment checkpoint.

---

## Execution Order Summary

| Step | Module(s) | Type | Est. Complexity | Tests Added |
|------|-----------|------|-----------------|-------------|
| 1 | `lib.rs`, `main.rs` | Structural | Low | 0 (enables all others) |
| 2 | `scan.rs` | Unit | Medium | ~15 tests |
| 3 | `install.rs` | Unit | Low | ~5 tests |
| 4 | `source.rs` | Unit | Medium | ~18 tests |
| 5 | `list.rs`, `search.rs` | Unit | Low | ~14 tests |
| 6 | `config.rs` | Unit | Low | ~6 tests |
| 7 | `types.rs` | Unit | Medium | ~12 tests |
| 8 | `lockfile.rs` | Unit | Low | ~2 tests |
| 9 | `outdated.rs` | Unit + Refactor | Medium | ~6 tests |
| 10 | `fs_util.rs`, `checksum.rs`, `diff.rs` | Refactor | Medium | 0 (dedup) |
| 11 | `tests/scan_integration.rs`, `tests/checksum_integration.rs` | Integration | Medium | ~12 tests |
| 12 | `lockfile.rs`, `tests/lockfile_integration.rs` | Refactor + Integration | Medium | ~4 tests |
| 13 | Coverage measurement | Assessment | Low | 0 |

**Total**: ~94 tests across 10 commits, progressing from zero coverage to comprehensive unit + integration coverage of all pure logic and critical filesystem paths.

## Key Design Decisions

- **Inline `mod tests` over separate test files for unit tests**: Private functions remain private. This is idiomatic Rust.
- **Extract `_str` variants rather than changing function signatures**: The I/O-wrapping functions (`parse_frontmatter`, `parse_agent_frontmatter`) keep their signatures stable. Tests target the extracted pure variants.
- **`tempfile` crate for integration tests**: Provides automatic cleanup and isolation. No risk of tests interfering with the user's real config.
- **No gateway traits yet**: Steps 2–12 extract maximum value without architectural refactoring. Gateway traits (for `FileSystem`, `GitClient`) are deferred until the pure-function tests are in place and the team is ready for a larger refactor.
- **No `mockall` or heavy mocking frameworks**: Use simple fakes and real temp directories instead. Keeps dev-dependencies minimal and tests grounded in real behavior.