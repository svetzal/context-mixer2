# cmx-core conformance fixtures

These fixtures are the language-neutral correctness contract for `cmx-core` ports.
Every case is generated from the Rust reference implementation with:

```bash
cargo run -p cmx-core --features test-support --bin generate-conformance-fixtures
```

The generator is a dedicated `test-support` binary so regeneration is explicit and reproducible, while the drift-guard test reuses the same library function in CI.

## Fixed environment

- Fixed clock: `2026-07-05T12:00:00+00:00`
- Virtual home: `/home/testuser`
- Global config root: `/home/testuser/.config/context-mixer`
- Local project root mapping: relative install paths are stored under `project/` in fixture trees

Tree snapshots use real files on disk. Absolute paths are stored without the leading `/`, so `/home/testuser/.claude/skills/fixture-tool/SKILL.md` appears as `home/testuser/.claude/skills/fixture-tool/SKILL.md`.

Lockfile expectations are stored as parsed JSON values, not byte-for-byte serialized text. Future ports must compare lockfiles as JSON values with sorted package keys.

## Layout

```text
cmx-core/conformance/
  README.md
  checksum/
  frontmatter/
  version-guard/
  paths/
  target-resolve/
  install-e2e/
```

Each category has one `manifest.json` that defines the schema for its cases. Inputs and expected outputs are separated explicitly.

## Category schemas

### `checksum/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "string-sort-a-dot-slash",
      "description": "human-readable note",
      "input": {
        "files": [
          { "path": "a", "content_utf8": "bare\n" }
        ]
      },
      "expected": {
        "sha256": "sha256:...",
        "canonical_order": ["a", "a.b", "a/b"],
        "canonical_included_paths": ["a", "a.b", "a/b"]
      }
    }
  ]
}
```

Notes:

- Checksum cases use inline UTF-8 file sets because some parity cases, such as `a` plus `a/b`, cannot exist simultaneously on a real filesystem tree.
- `canonical_order` and `canonical_included_paths` are the reference's filtered, sorted input to the hash.

### `frontmatter/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "existing-metadata-version",
      "description": "human-readable note",
      "input": {
        "version": "2.4.6",
        "skill_md_path": "existing-metadata-version/input/SKILL.md"
      },
      "expected": {
        "skill_md_path": "existing-metadata-version/expected/SKILL.md",
        "byte_exact": true,
        "idempotent_second_pass": false
      }
    }
  ]
}
```

The `input/` and `expected/` files are real `SKILL.md` byte fixtures. Ports must compare the expected output byte-for-byte.

### `version-guard/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "equal-drifted-skip",
      "description": "human-readable note",
      "input": {
        "bundled_version": "2.4.6",
        "tracked": true,
        "installed_version": "2.4.6",
        "disk_state": "drifted",
        "force": false
      },
      "expected": {
        "kind": "drifted-skip",
        "from": null,
        "installed": "2.4.6",
        "will_write": false,
        "blocked": false
      }
    }
  ]
}
```

`disk_state` is one of `missing`, `matches-source`, or `drifted`.

### `paths/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "copilot-global",
      "input": {
        "platform": "copilot",
        "kind": "skill",
        "scope": "global"
      },
      "expected": {
        "subpath": ".copilot/skills",
        "lockname": "cmx-lock-copilot.json"
      }
    }
  ]
}
```

This category covers every platform in `Platform::ALL` at both scopes.

### `target-resolve/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "fresh-machine",
      "description": "human-readable note",
      "input": {
        "scope": "global",
        "config_platforms": [],
        "non_empty_locks": []
      },
      "expected": {
        "resolved_platforms": ["claude"]
      }
    }
  ]
}
```

`non_empty_locks` lists the platforms whose scope-specific lockfiles were pre-populated before resolution.

### `install-e2e/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "fresh-install",
      "description": "human-readable note",
      "input": {
        "tool_name": "fixture-tool",
        "tool_version": "2.4.6",
        "scope": "global",
        "force": false,
        "bundle_dir": "fresh-install/bundle",
        "pre_tree_dir": "fresh-install/pre/tree",
        "pre_locks_dir": "fresh-install/pre/locks"
      },
      "expected": {
        "tree_dir": "fresh-install/expected/tree",
        "locks_dir": "fresh-install/expected/locks",
        "report_path": "fresh-install/expected/report.json"
      }
    }
  ]
}
```

Case contents:

- `bundle/` is the original bundled skill file set before frontmatter reconciliation.
- `pre/tree/` is the non-lock virtual filesystem tree before `plan`/`apply`.
- `pre/locks/` stores any pre-existing lockfiles as JSON values, keyed by lock filename.
- `expected/tree/` and `expected/locks/` are the post-apply filesystem state.
- `expected/report.json` contains:
  - `plan`: the observed plan snapshot from the Rust oracle
  - `apply.status`: `applied` or `blocked`
  - `apply.error`: present only for blocked runs
  - `apply.report`: the normalized Rust `Report` snapshot for successful applies

Ports should materialize `bundle/`, `pre/tree/`, and `pre/locks/` into an isolated test root, execute the equivalent operation, then compare the resulting tree, lock JSON values, and normalized report against `expected/`.

## Drift guard

`cargo test --workspace` includes a drift-guard test that regenerates this entire tree into a temp directory and compares it against the committed fixtures. JSON files are compared by parsed value; all other files are compared byte-for-byte.
