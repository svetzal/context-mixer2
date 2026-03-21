```json
{ "severity": 5, "principle": "All tests pass", "category": "Simple Design Heuristics" }
```

## Assessment: Zero Test Coverage in a Package Manager

### The Violation

The project's **#1 Simple Design Heuristic** — *"All tests pass — Correctness is non-negotiable"* — is vacuously satisfied. There are **zero tests** of any kind:

- 0 unit tests (`#[test]`)
- 0 async tests (`#[tokio::test]`)
- 0 integration tests (no `tests/` directory)
- 0 doctests (no executable examples in doc comments)
- 0 `mod tests` blocks across all 16 source files

The mandatory quality gate `cargo test --all-features` succeeds only because there is literally nothing to fail. This is the equivalent of a smoke detector with no batteries — the CI is green, but nothing is being verified.

### Why This Is Severity 5

This is a **package manager that handles artifact integrity** — SHA-256 checksums, lock files, version comparison, conflict detection, and recursive file operations. These are exactly the kinds of operations where subtle bugs cause data loss or silent corruption. Specifically, the following critical paths are completely unverified:

| Critical Path | Risk Without Tests |
|---|---|
| `checksum.rs` — SHA-256 computation for files and directories | Silent integrity failures if file ordering or path handling changes |
| `scan.rs` — Frontmatter parsing (YAML-like format) | Artifacts missed or misidentified on edge-case input |
| `install.rs` — Conflict detection, force-overwrite logic | Accidental data loss when `--force` interacts with local modifications |
| `lockfile.rs` — Lock file serialization round-trip | Lock corruption on upgrade, losing installation tracking |
| `outdated.rs` — Version/checksum comparison | False positives or missed updates |
| `types.rs` — Artifact enum, ArtifactKind display | Serialization mismatches between lock file and runtime |

### The Compounding Problem: No Functional Core

The reason tests don't exist is likely because the architecture makes them **hard to write**. I/O is pervasively mixed into business logic across every module. For example:

- `install.rs::install()` — 310 lines mixing artifact discovery, validation, file copying, and lock file management in a single function with inline `std::fs` calls
- `scan.rs` — Frontmatter parsing (pure logic) is interleaved with `fs::read_to_string()` and `fs::read_dir()`
- `source.rs` — Git operations via `std::process::Command` mixed with config persistence
- `diff.rs::collect_files_recursive()` duplicates `checksum.rs::collect_files()` — both do recursive filesystem walks with no shared abstraction

There are **zero local trait definitions** in the entire codebase. No filesystem gateway, no git client trait, no config store abstraction. The only trait-based boundary is `LlmGateway` from the external `mojentic` crate.

### How to Correct It

The fix is a two-phase approach: **extract testable logic first**, then **add gateway traits at I/O boundaries**.

#### Phase 1: Extract Pure Functions (immediate, high-value)

Several pieces of business logic can be extracted and tested *today* without any architectural refactoring:

1. **Frontmatter parsing** — `parse_frontmatter()` and `parse_agent_frontmatter()` in `scan.rs` can accept `&str` input instead of reading from disk. Unit-testable immediately.

2. **Checksum comparison logic** — The "is this artifact outdated?" decision in `outdated.rs` is pure comparison of strings and `Option<String>` values. Extract and test.

3. **Version/status indicators** — `status_indicator()` in `list.rs` and the matching logic in `outdated.rs` are pure functions hiding inside I/O-coupled modules.

4. **Lock entry construction** — Building a `LockEntry` from inputs is pure data transformation. Test the round-trip (construct → serialize → deserialize → compare).

5. **Artifact name resolution** — The logic for resolving `source/name` syntax vs bare names in `install.rs` is pure string parsing.

#### Phase 2: Introduce Gateway Traits (architectural)

For the I/O-heavy operations, introduce thin gateway traits:

```rust
// Trait for filesystem operations — thin wrapper, no logic to test
trait FileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String>;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;
    fn copy(&self, src: &Path, dst: &Path) -> Result<()>;
    fn write(&self, path: &Path, content: &[u8]) -> Result<()>;
    fn remove(&self, path: &Path) -> Result<()>;
}

// Trait for git operations
trait GitClient {
    fn clone(&self, url: &str, dest: &Path, branch: Option<&str>) -> Result<()>;
    fn pull(&self, repo: &Path) -> Result<()>;
}
```

This lets you write in-memory fakes for testing the core logic (scan, install, update, outdated) without touching the real filesystem.

#### Phase 3: Add a `lib.rs`

The project is binary-only (`main.rs`, no `lib.rs`). Extracting a library target would enable:
- Integration tests in `tests/`
- Doctests on public API items
- `cargo test --lib` and `cargo test --doc` as meaningful checks

### Priority Order

1. **Add `lib.rs`** and re-export modules (structural change, enables everything else)
2. **Extract pure parsing/comparison functions** and add unit tests (Phase 1 — immediate wins)
3. **Add serde round-trip tests** for `LockFile`, `SourcesFile`, `CmxConfig` types
4. **Introduce `FileSystem` gateway trait** and test `scan.rs` with in-memory fakes (Phase 2)
5. **Deduplicate `collect_files()`** between `checksum.rs` and `diff.rs` (also fixes a knowledge duplication violation)
6. **Add integration tests** for end-to-end install/uninstall/outdated workflows using temp directories

This project has solid bones — clean types, consistent error handling, no unsafe code, well-configured CI. But the total absence of tests means **there is no executable specification of correctness**, which is the single most important quality a codebase can have.