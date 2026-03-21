All 106 tests pass, all quality gates are green. The LSP diagnostics shown to you were stale cache from before the agent's edits — the actual compiler sees no issues.

Here's what was accomplished across the three commits:

---

### What changed and why

**Step 1 — `checksum_artifact` + `installed_path`**

The decision *"agents are files; skills are directories"* was scattered across 10+ `match kind` blocks in 5 files. It now lives in exactly two functions:
- `checksum::checksum_artifact(path, kind)` — the single place that dispatches file vs. directory checksumming
- `ArtifactKind::installed_path(name, dir)` — the single place that knows agents get `.md` appended and skills use a bare directory name

**Step 2 — Flattened `Artifact` struct + unified `ArtifactKind`**

The `Artifact` enum with two structurally identical variants was eliminated. It's now a plain struct with a `kind: ArtifactKind` field. This removed 7 accessor methods that were all identical match arms (`Agent => value, Skill => value`). The `ArtifactKindSerde` redundant type was also removed — `ArtifactKind` now derives `Serialize/Deserialize` directly with `#[serde(rename_all = "lowercase")]`.

**Step 3 — `source_iter` module**

The traversal pattern *"load sources → resolve local path → skip if missing → scan → iterate artifacts"* was duplicated verbatim across 6 files. It now lives in `source_iter::each_source_artifact(sources)`, which all callers use with a simple `for sa in source_iter::each_source_artifact(...)` loop. Adding a new source type, changing traversal order, or adding cross-cutting filtering is now a one-place change.