```json
{ "severity": 3, "principle": "No knowledge duplication", "category": "Simple Design Heuristics" }
```

## Assessment

The codebase is well-structured for its size — all quality gates pass (fmt, clippy, tests, deny), there are 100 tests covering pure functions thoroughly, and the project invariants (edition 2024, toolchain 1.94.0, tokio runtime) are correctly pinned. Good work here. The code is readable and the intent is clear throughout.

However, there is one principle the codebase violates significantly: **"No knowledge duplication — avoid multiple spots that must change together for the same reason."**

### The Violation: A Single Decision Repeated 10+ Times

The decision *"agents are single files; skills are directories"* is encoded as a `match kind` block in **10 separate locations** across 4 source files:

```rust
// This exact decision pattern appears 10 times:
match kind {
    ArtifactKind::Agent => checksum::checksum_file(path)?,
    ArtifactKind::Skill => checksum::checksum_dir(path)?,
}
```

**Occurrences:** `install.rs` (×5), `diff.rs` (×2), `outdated.rs` (×2), `info.rs` (×1)

If the checksum strategy for either kind changes — or a third kind is added — all 10 locations must be found and updated in lockstep. This is the textbook definition of duplicated *knowledge*, not just duplicated *code*.

### A second layer: the source-traversal loop

The pattern `load_sources → iterate → resolve_local_path → check exists → scan_source → iterate artifacts` is repeated across **8 functions** in `install.rs`, `diff.rs`, `list.rs`, `outdated.rs`, `search.rs`, and `info.rs`. Each reimplements the same traversal with slight variations in what it does with the found artifacts.

### A third layer: the `Artifact` enum itself

`Artifact::Agent` and `Artifact::Skill` have **structurally identical fields** (name, description, path, version, deprecation). All 7 accessor methods do the same thing for both variants:

```rust
pub fn name(&self) -> &str {
    match self {
        Artifact::Agent { name, .. } => name,
        Artifact::Skill { name, .. } => name,  // identical
    }
}
// × 7 accessors
```

This is an enum pretending to be a struct with a `kind` field. A `struct Artifact { kind: ArtifactKind, name: String, ... }` would eliminate all 7 match arms and make the code shorter and easier to extend.

### How to Correct It

**Step 1 — Extract `checksum_artifact`** (smallest, highest-impact change):

```rust
// In checksum.rs
pub fn checksum_artifact(path: &Path, kind: ArtifactKind) -> Result<String> {
    match kind {
        ArtifactKind::Agent => checksum_file(path),
        ArtifactKind::Skill => checksum_dir(path),
    }
}
```

This collapses 10 match blocks into 10 single-line calls, and the decision lives in one place.

**Step 2 — Flatten `Artifact` to a struct:**

```rust
pub struct Artifact {
    pub kind: ArtifactKind,
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub version: Option<String>,
    pub deprecation: Option<Deprecation>,
}
```

Eliminates all 7 match-arm accessors and makes the type trivially extensible.

**Step 3 — Extract a source-traversal helper** (moderate refactor):

```rust
pub fn for_each_source_artifact<F>(kind: Option<ArtifactKind>, mut f: F) -> Result<()>
where F: FnMut(&str, &Artifact, &Path) -> Result<()>
```

This would deduplicate the 8 traversal loops into one reusable iteration function that the callers simply provide a closure to.

### What's Working Well

- Pure helper functions are well-extracted and thoroughly tested (`parse_name`, `is_stale`, `is_outdated`, `status_indicator`, `truncate_description`, frontmatter parsing)
- Quality gates are in place and passing
- Error handling is consistent (anyhow with context)
- Lock file and checksum logic are solid with good integration tests
- No unsafe code, no panics in library code

The recommended fix order is Step 1 → Step 2 → Step 3, each independently valuable and safe to land as a separate commit.