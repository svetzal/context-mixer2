Now I have the complete picture. Here is the detailed, step-by-step plan:

---

# Refactoring Plan: Eliminate Knowledge Duplication in cmx

## Project Invariants (Do Not Change)
- Edition: **2024**
- Toolchain: pinned (check `rust-toolchain.toml`)
- Async runtime: **tokio**
- No MSRV declared (binary crate)

## Quality Gates (Run After Every Step)
```bash
cargo fmt --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features && \
cargo deny check
```

---

## Step 1: Extract `checksum_artifact` helper (smallest, highest-impact)

### 1.1 — Add `checksum_artifact` to `src/checksum.rs`

Add a new public function at the bottom of `src/checksum.rs`:

```rust
use crate::types::ArtifactKind;

/// Compute the checksum for an artifact, dispatching to the correct strategy
/// based on its kind: file checksum for agents, directory checksum for skills.
pub fn checksum_artifact(path: &Path, kind: ArtifactKind) -> Result<String> {
    match kind {
        ArtifactKind::Agent => checksum_file(path),
        ArtifactKind::Skill => checksum_dir(path),
    }
}
```

This is the **single place** the decision "agents are files; skills are directories" lives for checksumming.

### 1.2 — Replace all 8 checksum dispatch match blocks

Each of the following match blocks becomes a single-line call to `checksum::checksum_artifact(path, kind)`:

**`src/install.rs` — lines 68-71** (source checksum before copy):
```rust
// Before:
let source_checksum = match &artifact {
    Artifact::Agent { path, .. } => checksum::checksum_file(path)?,
    Artifact::Skill { path, .. } => checksum::checksum_dir(path)?,
};
// After:
let source_checksum = checksum::checksum_artifact(artifact.path(), kind)?;
```

**`src/install.rs` — lines 90-93** (local modification check):
```rust
// Before:
let current_cs = match kind {
    ArtifactKind::Agent => checksum::checksum_file(&dest_check)?,
    ArtifactKind::Skill => checksum::checksum_dir(&dest_check)?,
};
// After:
let current_cs = checksum::checksum_artifact(&dest_check, kind)?;
```

**`src/install.rs` — lines 131-134** (installed checksum after copy):
```rust
// Before:
let installed_checksum = match kind {
    ArtifactKind::Agent => checksum::checksum_file(&dest_path)?,
    ArtifactKind::Skill => checksum::checksum_dir(&dest_path)?,
};
// After:
let installed_checksum = checksum::checksum_artifact(&dest_path, kind)?;
```

**`src/install.rs` — lines 200-203 (in `install_all`)** and **lines 272-274 (in `scan_source_checksums`)**: Both become:
```rust
let cs = checksum::checksum_artifact(artifact.path(), kind)?;
```
Note: in `install_all` (line 200), the variable is `source_cs`; in `scan_source_checksums` (line 272), the variable is `cs`. Preserve the local variable names.

**`src/diff.rs` — lines 26-33** (two match blocks for installed and source checksum):
```rust
// Before:
let installed_checksum = match kind {
    ArtifactKind::Agent => checksum::checksum_file(&installed_path)?,
    ArtifactKind::Skill => checksum::checksum_dir(&installed_path)?,
};
let source_checksum = match kind {
    ArtifactKind::Agent => checksum::checksum_file(&source_path)?,
    ArtifactKind::Skill => checksum::checksum_dir(&source_path)?,
};
// After:
let installed_checksum = checksum::checksum_artifact(&installed_path, kind)?;
let source_checksum = checksum::checksum_artifact(&source_path, kind)?;
```

**`src/outdated.rs` — lines 175-178** (local modification check in `collect_outdated_for_scope`):
```rust
// Before:
let current_cs = match kind {
    ArtifactKind::Agent => checksum::checksum_file(&install_path)?,
    ArtifactKind::Skill => checksum::checksum_dir(&install_path)?,
};
// After:
let current_cs = checksum::checksum_artifact(&install_path, kind)?;
```

**`src/outdated.rs` — lines 215-218** (in `scan_all_sources`):
```rust
// Before:
let cs = match artifact.artifact_kind() {
    ArtifactKind::Agent => checksum::checksum_file(artifact.path())?,
    ArtifactKind::Skill => checksum::checksum_dir(artifact.path())?,
};
// After:
let cs = checksum::checksum_artifact(artifact.path(), artifact.artifact_kind())?;
```

**`src/info.rs` — lines 50-53** (local modification check):
```rust
// Before:
let current_checksum = match kind {
    ArtifactKind::Agent => checksum::checksum_file(path)?,
    ArtifactKind::Skill => checksum::checksum_dir(path)?,
};
// After:
let current_checksum = checksum::checksum_artifact(path, kind)?;
```

### 1.3 — Extract `installed_artifact_path` helper

While touching these files, also extract the path-construction decision ("agents are `{name}.md` files; skills are `{name}` directories") which is repeated 5 times. Add a method to `ArtifactKind` in `src/types.rs`:

```rust
impl ArtifactKind {
    /// Compute the expected filesystem path for an installed artifact within a
    /// given install directory.
    pub fn installed_path(&self, name: &str, dir: &Path) -> PathBuf {
        match self {
            ArtifactKind::Agent => dir.join(format!("{name}.md")),
            ArtifactKind::Skill => dir.join(name),
        }
    }
}
```

Then replace the 5 occurrences:

- **`src/install.rs` lines 83-86** → `let dest_check = kind.installed_path(artifact_name, &dest_dir);`
- **`src/diff.rs` lines 75-78** (in `find_installed_on_disk`) → `let path = kind.installed_path(name, &dir);`
- **`src/uninstall.rs` lines 11-14** → `let target = kind.installed_path(name, &dir);`
- **`src/info.rs` lines 17-20** → `let path = kind.installed_path(name, &dir);`
- **`src/outdated.rs` lines 170-173** → `let install_path = kind.installed_path(name, &config::install_dir(kind, local)?);`

### 1.4 — Add unit test for `checksum_artifact`

In `tests/checksum_integration.rs`, add:

```rust
use cmx::checksum::checksum_artifact;
use cmx::types::ArtifactKind;

#[test]
fn checksum_artifact_dispatches_file_for_agent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("agent.md");
    fs::write(&path, b"agent content").unwrap();
    let cs = checksum_artifact(&path, ArtifactKind::Agent).unwrap();
    let expected = checksum_file(&path).unwrap();
    assert_eq!(cs, expected);
}

#[test]
fn checksum_artifact_dispatches_dir_for_skill() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("SKILL.md"), b"# Skill\n").unwrap();
    let cs = checksum_artifact(dir.path(), ArtifactKind::Skill).unwrap();
    let expected = checksum_dir(dir.path()).unwrap();
    assert_eq!(cs, expected);
}
```

### 1.5 — Add unit test for `installed_path`

In `src/types.rs` `mod tests`, add:

```rust
#[test]
fn installed_path_agent_appends_md_extension() {
    let dir = Path::new("/home/user/.claude/agents");
    let path = ArtifactKind::Agent.installed_path("my-agent", dir);
    assert_eq!(path, PathBuf::from("/home/user/.claude/agents/my-agent.md"));
}

#[test]
fn installed_path_skill_uses_bare_name() {
    let dir = Path::new("/home/user/.claude/skills");
    let path = ArtifactKind::Skill.installed_path("my-skill", dir);
    assert_eq!(path, PathBuf::from("/home/user/.claude/skills/my-skill"));
}
```

### 1.6 — Run quality gates

```bash
cargo fmt --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features && \
cargo deny check
```

Fix any issues. Commit with message: `Extract checksum_artifact and installed_path to eliminate kind-dispatch duplication`

**Result:** 8 checksum match blocks → 8 one-line calls. 5 path-construction match blocks → 5 one-line calls. The decisions "agents=files, skills=dirs" and "agents=name.md, skills=name" each live in exactly one place.

---

## Step 2: Flatten `Artifact` enum to a struct

### 2.1 — Change the `Artifact` type definition in `src/types.rs`

Replace the current enum (lines 90-106):

```rust
#[derive(Debug)]
pub enum Artifact {
    Agent { name: String, description: String, path: PathBuf, version: Option<String>, deprecation: Option<Deprecation> },
    Skill { name: String, description: String, path: PathBuf, version: Option<String>, deprecation: Option<Deprecation> },
}
```

With a struct:

```rust
#[derive(Debug)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub version: Option<String>,
    pub deprecation: Option<Deprecation>,
}
```

### 2.2 — Simplify the `Artifact` impl block (lines 160-213)

Replace the entire impl block. The 7 match-arm accessors become trivial field access. Most accessors can be removed entirely since fields are now `pub`. Keep only derived convenience methods:

```rust
impl Artifact {
    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}
```

The following accessors are **removed** because they become direct field access:
- `name()` → `artifact.name` (or `&artifact.name`)
- `description()` → `artifact.description`
- `kind()` → use `artifact.kind.to_string()` or `format!("{}", artifact.kind)`
- `artifact_kind()` → `artifact.kind`
- `path()` → `&artifact.path`
- `version()` → `artifact.version.as_deref()`
- `deprecation()` → `artifact.deprecation.as_ref()`

### 2.3 — Update all callers of the old accessor methods

This is a mechanical find-and-replace across the codebase. The key transformations:

| Old call | New expression |
|---|---|
| `artifact.name()` | `&artifact.name` or `artifact.name.as_str()` |
| `artifact.description()` | `&artifact.description` |
| `artifact.kind()` | `artifact.kind.to_string()` (only used in `search.rs` line 38) |
| `artifact.artifact_kind()` | `artifact.kind` |
| `artifact.path()` | `&artifact.path` |
| `artifact.version()` | `artifact.version.as_deref()` |
| `artifact.deprecation()` | `artifact.deprecation.as_ref()` |
| `artifact.is_deprecated()` | `artifact.is_deprecated()` (unchanged, keep this method) |

**Files to update (all accessor call sites):**

- **`src/install.rs`**: lines 43 (`artifact.name()`, `artifact.artifact_kind()`), line 68 (already replaced in Step 1 — now `artifact.path()` → `&artifact.path`), lines 74-78 (`artifact.path()`), lines 104-118 (the copy match block — see 2.4 below), line 145 (`artifact.version()`), line 157 (`artifact.version()`), line 195 (`artifact.artifact_kind()`), line 200 (`artifact.path()`), line 271 (`artifact.artifact_kind()`), line 273 (`artifact.path()`), line 276 (`artifact.name()`)
- **`src/diff.rs`**: lines 97-101 (`artifact.name()`, `artifact.artifact_kind()`, `artifact.path()`, `artifact.version()`)
- **`src/outdated.rs`**: lines 215-224 (`artifact.artifact_kind()`, `artifact.path()`, `artifact.name()`, `artifact.version()`)
- **`src/list.rs`**: lines 141-149 (`artifact.artifact_kind()`, `artifact.version()`, `artifact.is_deprecated()`, `artifact.name()`)
- **`src/search.rs`**: lines 29-41 (`artifact.name()`, `artifact.description()`, `artifact.kind()`, `artifact.version()`)
- **`src/info.rs`**: lines 71-87 (`artifact.name()`, `artifact.artifact_kind()`, `artifact.deprecation()`, `artifact.version()`)
- **`src/scan.rs`**: line 18 (`a.name()` in sort)
- **`src/source.rs`**: search for any `artifact.name()` or similar calls (likely in the `browse` subcommand)

### 2.4 — Update the copy block in `install.rs` (lines 104-119)

This is the one match on `&artifact` that does genuinely different work (file copy vs dir copy). It should now match on `kind` (which is already in scope):

```rust
let dest_path = match kind {
    ArtifactKind::Agent => {
        let filename = artifact.path.file_name().context("Invalid agent path")?;
        let dest = dest_dir.join(filename);
        fs::copy(&artifact.path, &dest).with_context(|| {
            format!("Failed to copy {} to {}", artifact.path.display(), dest.display())
        })?;
        dest
    }
    ArtifactKind::Skill => {
        let dir_name = artifact.path.file_name().context("Invalid skill path")?;
        let dest = dest_dir.join(dir_name);
        copy_dir_recursive(&artifact.path, &dest)?;
        dest
    }
};
```

### 2.5 — Update `scan.rs` artifact construction

All `Artifact::Agent { ... }` and `Artifact::Skill { ... }` constructors become `Artifact { kind: ArtifactKind::Agent, ... }` and `Artifact { kind: ArtifactKind::Skill, ... }`.

**Locations in `src/scan.rs`:**
- `scan_marketplace` line 47: `Artifact::Agent { name, description, path, version, deprecation }` → `Artifact { kind: ArtifactKind::Agent, name, description: fm.description, path: full_path, version: fm.version, deprecation: fm.deprecation }`
- `scan_marketplace` line 84: `Artifact::Skill { ... }` → `Artifact { kind: ArtifactKind::Skill, ... }`
- `walk_dir` line 123: `Artifact::Skill { ... }` → `Artifact { kind: ArtifactKind::Skill, ... }`
- `walk_dir` line 137: `Artifact::Agent { ... }` → `Artifact { kind: ArtifactKind::Agent, ... }`

Add `use crate::types::ArtifactKind;` to `scan.rs` imports.

### 2.6 — Unify `ArtifactKind` and `ArtifactKindSerde`

Since we're restructuring `Artifact`, this is the natural time to eliminate the redundant `ArtifactKindSerde` enum. Currently `ArtifactKind` lacks serde derives, and `ArtifactKindSerde` exists solely for serialization. Unify them:

Add serde derives to `ArtifactKind`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Agent,
    Skill,
}
```

Then:
- **Remove** the `ArtifactKindSerde` enum entirely (lines 138-143)
- **Change** `LockEntry.artifact_type` from `ArtifactKindSerde` to `ArtifactKind`
- **Remove** the conversion match in `install.rs` lines 141-144 (`ArtifactKind::Agent => ArtifactKindSerde::Agent`). Just use `kind` directly:
  ```rust
  artifact_type: kind,
  ```
- **Remove** the conversion match in `install.rs` lines 234-237 (`ArtifactKindSerde::Agent => ArtifactKind::Agent`). Just use `entry.artifact_type` directly:
  ```rust
  let entry_kind = entry.artifact_type;
  if entry_kind != kind { continue; }
  ```
  Or inline: `if entry.artifact_type != kind { continue; }`
- **Update** `src/outdated.rs` test helper `make_lock_entry` (line 241): `ArtifactKindSerde::Agent` → `ArtifactKind::Agent`
- **Update** `src/types.rs` test helpers: all `ArtifactKindSerde::Agent` → `ArtifactKind::Agent`
- **Update** `tests/lockfile_integration.rs`: replace `ArtifactKindSerde` imports and usages with `ArtifactKind`
- **Update** the golden JSON test (line 425): `matches!(entry.artifact_type, ArtifactKindSerde::Agent)` → `assert_eq!(entry.artifact_type, ArtifactKind::Agent)`

### 2.7 — Update tests in `src/types.rs`

The test helper functions `make_agent()` and `make_skill()` (lines 351-372) must be updated from enum variant construction to struct construction:

```rust
fn make_agent() -> Artifact {
    Artifact {
        kind: ArtifactKind::Agent,
        name: "test-agent".to_string(),
        description: "Agent description".to_string(),
        path: PathBuf::from("test-agent.md"),
        version: Some("2.0.0".to_string()),
        deprecation: None,
    }
}

fn make_skill() -> Artifact {
    Artifact {
        kind: ArtifactKind::Skill,
        name: "test-skill".to_string(),
        description: "Skill description".to_string(),
        path: PathBuf::from("test-skill"),
        version: Some("1.0.0".to_string()),
        deprecation: Some(Deprecation {
            reason: Some("Old".to_string()),
            replacement: Some("new-skill".to_string()),
        }),
    }
}
```

Update the accessor tests to use direct field access instead of methods:

```rust
#[test]
fn artifact_agent_fields() {
    let a = make_agent();
    assert_eq!(a.name, "test-agent");
    assert_eq!(a.description, "Agent description");
    assert_eq!(a.kind, ArtifactKind::Agent);
    assert_eq!(a.path, Path::new("test-agent.md"));
    assert_eq!(a.version.as_deref(), Some("2.0.0"));
    assert!(!a.is_deprecated());
    assert!(a.deprecation.is_none());
}

#[test]
fn artifact_skill_fields() {
    let s = make_skill();
    assert_eq!(s.name, "test-skill");
    assert_eq!(s.description, "Skill description");
    assert_eq!(s.kind, ArtifactKind::Skill);
    assert_eq!(s.path, Path::new("test-skill"));
    assert_eq!(s.version.as_deref(), Some("1.0.0"));
    assert!(s.is_deprecated());
    let dep = s.deprecation.as_ref().unwrap();
    assert_eq!(dep.reason.as_deref(), Some("Old"));
    assert_eq!(dep.replacement.as_deref(), Some("new-skill"));
}
```

### 2.8 — Update integration tests

- **`tests/scan_integration.rs`**: Any test that pattern-matches on `Artifact::Agent { .. }` or `Artifact::Skill { .. }` must instead check `artifact.kind == ArtifactKind::Agent` and access fields directly.
- **`tests/lockfile_integration.rs`**: Replace `ArtifactKindSerde` with `ArtifactKind` in all type annotations and assertions.

### 2.9 — Run quality gates

```bash
cargo fmt --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features && \
cargo deny check
```

Fix any issues. Commit with message: `Flatten Artifact enum to struct and unify ArtifactKind/ArtifactKindSerde`

**Result:** 7 match-arm accessors eliminated. `ArtifactKindSerde` eliminated. The `Artifact` type is now a simple struct that's trivially extensible — adding a new field requires zero match-arm changes.

---

## Step 3: Extract source-traversal helper

### 3.1 — Identify the common pattern

The repeated pattern across 7+ functions is:

```rust
let sources = config::load_sources()?;
for (source_name, entry) in &sources.sources {
    let local_path = config::resolve_local_path(entry);
    if !local_path.exists() {
        continue;
    }
    if let Ok(artifacts) = scan::scan_source(&local_path) {
        for artifact in &artifacts {
            // ... caller-specific logic using (source_name, artifact, local_path)
        }
    }
}
```

### 3.2 — Create a new module `src/source_iter.rs`

Create a new file `src/source_iter.rs` with a helper that encapsulates the traversal:

```rust
use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::scan;
use crate::types::{Artifact, SourcesFile};

/// A scanned artifact with its source context.
pub struct SourceArtifact {
    pub source_name: String,
    pub source_root: PathBuf,
    pub artifact: Artifact,
}

/// Iterate over all artifacts across all registered sources.
///
/// Loads sources, resolves local paths, scans each source, and yields
/// every artifact found with its source context. Silently skips sources
/// whose local paths don't exist or that fail to scan.
pub fn each_source_artifact(sources: &SourcesFile) -> Result<Vec<SourceArtifact>> {
    let mut results = Vec::new();

    for (source_name, entry) in &sources.sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in artifacts {
                results.push(SourceArtifact {
                    source_name: source_name.clone(),
                    source_root: local_path.clone(),
                    artifact,
                });
            }
        }
    }

    Ok(results)
}
```

**Design note:** This returns a `Vec` rather than an iterator because `scan_source` is fallible and the lifetimes would be complex with a lending iterator. The vec is fine — source repos are small (dozens of artifacts, not millions).

**Alternative considered:** A closure-based `for_each_source_artifact(sources, |source_name, artifact, local_path| { ... })` approach. This would avoid collecting into a vec but makes early-return from callers harder. The vec approach is simpler and idiomatic for this scale.

### 3.3 — Register the module in `src/lib.rs`

Add `pub mod source_iter;` to `src/lib.rs`.

### 3.4 — Refactor `src/install.rs` — `install()` function (lines 36-47)

```rust
// Before: manual loop
for (sname, entry) in &search_sources { ... }

// After:
let all_artifacts = source_iter::each_source_artifact(&SourcesFile { version: 1, sources: search_sources.into_iter().collect() })?;
for sa in all_artifacts {
    if sa.artifact.name == artifact_name && sa.artifact.kind == kind {
        found.push((sa.source_name, sa.artifact, sa.source_root));
    }
}
```

**Hmm — actually**, the `install()` function is special because it filters `search_sources` (a subset of all sources) based on the optional `source:name` prefix. The helper takes a `&SourcesFile`. We should adjust the helper to accept a `&SourcesFile` so callers can pass either the full sources or a filtered subset. Looking at the code again, `install()` already constructs a filtered `search_sources: Vec<(String, SourceEntry)>`. We can construct a temporary `SourcesFile` from that, or better — make the helper accept `&BTreeMap<String, SourceEntry>` instead of `&SourcesFile`:

```rust
pub fn each_source_artifact(
    sources: &BTreeMap<String, SourceEntry>,
) -> Result<Vec<SourceArtifact>> {
```

This is more flexible and avoids constructing a temporary `SourcesFile`.

### 3.5 — Refactor `src/install.rs` — `install_all()` function (lines 188-215)

```rust
// Before: manual loop with per-artifact checksum check
// After:
let all = source_iter::each_source_artifact(&sources.sources)?;
for sa in &all {
    if sa.artifact.kind != kind {
        continue;
    }
    if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
        let source_cs = checksum::checksum_artifact(&sa.artifact.path, kind)?;
        if lock_entry.version.as_deref() == sa.artifact.version.as_deref()
            && lock_entry.source_checksum == source_cs
        {
            continue;
        }
    }
    let pinned = format!("{}:{}", sa.source_name, sa.artifact.name);
    install(&pinned, kind, local, force)?;
    installed_count += 1;
}
```

### 3.6 — Refactor `src/install.rs` — `scan_source_checksums()` function (lines 259-283)

```rust
fn scan_source_checksums(kind: ArtifactKind) -> Result<BTreeMap<String, String>> {
    let sources = config::load_sources()?;
    let all = source_iter::each_source_artifact(&sources.sources)?;
    let mut checksums = BTreeMap::new();

    for sa in &all {
        if sa.artifact.kind == kind {
            let cs = checksum::checksum_artifact(&sa.artifact.path, kind)?;
            checksums.insert(sa.artifact.name.clone(), cs);
        }
    }

    Ok(checksums)
}
```

### 3.7 — Refactor `src/diff.rs` — `find_in_sources()` function (lines 87-109)

```rust
fn find_in_sources(name: &str, kind: ArtifactKind) -> Result<(PathBuf, String, Option<String>)> {
    let sources = config::load_sources()?;
    let all = source_iter::each_source_artifact(&sources.sources)?;

    for sa in all {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            return Ok((
                sa.artifact.path,
                sa.source_name,
                sa.artifact.version,
            ));
        }
    }

    bail!("No {kind} named '{name}' found in any registered source.");
}
```

### 3.8 — Refactor `src/outdated.rs` — `scan_all_sources()` function (lines 204-232)

```rust
fn scan_all_sources() -> Result<BTreeMap<String, SourceArtifactInfo>> {
    let sources = config::load_sources()?;
    let all = source_iter::each_source_artifact(&sources.sources)?;
    let mut result = BTreeMap::new();

    for sa in &all {
        let cs = checksum::checksum_artifact(&sa.artifact.path, sa.artifact.kind)?;
        result.insert(
            sa.artifact.name.clone(),
            SourceArtifactInfo {
                source_name: sa.source_name.clone(),
                version: sa.artifact.version.clone(),
                checksum: cs,
            },
        );
    }

    Ok(result)
}
```

### 3.9 — Refactor `src/list.rs` — `build_source_versions()` function (lines 130-158)

```rust
fn build_source_versions(kind: ArtifactKind) -> Result<BTreeMap<String, SourceInfo>> {
    let sources = config::load_sources()?;
    let all = source_iter::each_source_artifact(&sources.sources)?;
    let mut versions = BTreeMap::new();

    for sa in all {
        if sa.artifact.kind == kind {
            let version = sa.artifact.version.as_deref().unwrap_or("-").to_string();
            let deprecated = sa.artifact.is_deprecated();
            versions.insert(
                sa.artifact.name,
                SourceInfo {
                    source_name: sa.source_name,
                    version,
                    deprecated,
                },
            );
        }
    }

    Ok(versions)
}
```

### 3.10 — Refactor `src/search.rs` — `search()` function (lines 22-46)

```rust
let all = source_iter::each_source_artifact(&sources.sources)?;
for sa in &all {
    let name_lower = sa.artifact.name.to_lowercase();
    let desc_lower = sa.artifact.description.to_lowercase();

    if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
        let short_desc = truncate_description(&sa.artifact.description, 80);
        results.push(SearchResult {
            name: sa.artifact.name.clone(),
            kind: sa.artifact.kind.to_string(),
            version: sa.artifact.version.as_deref().unwrap_or("-").to_string(),
            source: sa.source_name.clone(),
            description: short_desc,
        });
    }
}
```

### 3.11 — Refactor `src/info.rs` — source scanning in `show_info()` (lines 63-91)

```rust
source::auto_update_all().ok();
let sources = config::load_sources()?;
let all = source_iter::each_source_artifact(&sources.sources)?;
for sa in &all {
    if sa.artifact.name == name && sa.artifact.kind == kind {
        if let Some(dep) = &sa.artifact.deprecation {
            println!("Status:      DEPRECATED");
            if let Some(reason) = &dep.reason {
                println!("  Reason:    {reason}");
            }
            if let Some(repl) = &dep.replacement {
                println!("  Replace:   {repl}");
            }
        }
        if let Some(v) = &sa.artifact.version {
            let installed_v =
                lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-");
            if v != installed_v {
                println!("Available:   v{v} (update available)");
            }
        }
    }
}
```

### 3.12 — Check `src/source.rs` for similar patterns

Read `src/source.rs` to check if the `browse` subcommand also contains this loop pattern. If so, refactor it the same way.

### 3.13 — Add unit test for `each_source_artifact`

In `src/source_iter.rs`, add a `#[cfg(test)] mod tests` block. Since the function does I/O (filesystem checks, scanning), test it with tempdir fixtures:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SourceEntry, SourceType};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn each_source_artifact_skips_missing_paths() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "missing".to_string(),
            SourceEntry {
                source_type: SourceType::Local,
                path: Some(PathBuf::from("/nonexistent/path")),
                url: None,
                local_clone: None,
                branch: None,
                last_updated: None,
            },
        );
        let result = each_source_artifact(&sources).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn each_source_artifact_finds_agents() {
        let dir = TempDir::new().unwrap();
        let agent_path = dir.path().join("my-agent.md");
        fs::write(&agent_path, "---\nname: my-agent\ndescription: Test\n---\n# Agent").unwrap();

        let mut sources = BTreeMap::new();
        sources.insert(
            "test-source".to_string(),
            SourceEntry {
                source_type: SourceType::Local,
                path: Some(dir.path().to_path_buf()),
                url: None,
                local_clone: None,
                branch: None,
                last_updated: None,
            },
        );

        let result = each_source_artifact(&sources).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_name, "test-source");
        assert_eq!(result[0].artifact.name, "my-agent");
        assert_eq!(result[0].artifact.kind, ArtifactKind::Agent);
    }
}
```

### 3.14 — Run quality gates

```bash
cargo fmt --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all-features && \
cargo deny check
```

Fix any issues. Commit with message: `Extract source_iter module to deduplicate source-traversal loops`

**Result:** 7 source-traversal loops replaced with calls to one shared helper. Adding a new source type, changing the traversal order, or adding filtering logic requires changing only one function.

---

## Summary of Changes by File

| File | Step 1 | Step 2 | Step 3 |
|---|---|---|---|
| `src/types.rs` | Add `installed_path` method on `ArtifactKind` + tests | Flatten `Artifact` to struct, remove `ArtifactKindSerde`, add serde to `ArtifactKind`, update tests | — |
| `src/checksum.rs` | Add `checksum_artifact` function | — | — |
| `src/source_iter.rs` | — | — | New module (created) |
| `src/lib.rs` | — | — | Add `pub mod source_iter` |
| `src/install.rs` | Replace 4 match blocks | Update accessor calls, update copy block | Use `each_source_artifact` in 3 functions |
| `src/diff.rs` | Replace 2 match blocks | Update accessor calls | Use `each_source_artifact` in `find_in_sources` |
| `src/outdated.rs` | Replace 2 match blocks | Update accessor calls, update test helper | Use `each_source_artifact` in `scan_all_sources` |
| `src/info.rs` | Replace 1 match block + 1 path block | Update accessor calls | Use `each_source_artifact` in `show_info` |
| `src/list.rs` | — | Update accessor calls | Use `each_source_artifact` in `build_source_versions` |
| `src/search.rs` | — | Update accessor calls | Use `each_source_artifact` in `search` |
| `src/uninstall.rs` | Replace 1 path block | — | — |
| `src/scan.rs` | — | Update `Artifact` constructors | — |
| `tests/checksum_integration.rs` | Add 2 tests | — | — |
| `tests/scan_integration.rs` | — | Update pattern matches | — |
| `tests/lockfile_integration.rs` | — | Replace `ArtifactKindSerde` with `ArtifactKind` | — |

## Risk Assessment

- **Step 1** — Very low risk. Purely additive (new function), then mechanical replacement. All existing tests continue to pass with no behavioral change.
- **Step 2** — Moderate risk. Touches every file that references `Artifact`. The change is mechanical but broad. Compiler will catch all breakage since enum variants disappear. Run tests carefully.
- **Step 3** — Low-moderate risk. Creates a new module and rewires callers. The behavioral change is nil (same traversal logic, just centralized). The main risk is subtle differences between callers that look identical but aren't — verify each caller carefully against its replacement.

## Commit Strategy

Three separate commits, one per step. Each commit is independently valuable and the codebase compiles and passes all tests after each one. If time is limited, Step 1 alone provides the biggest bang-for-buck improvement.