# cmv Command Reference

cmv verifies, deterministically, that a project holds the intents cmf compiled
for it. It reads the compile manifest `cmf install --local` wrote, resolves
each compiled intent's record in the knowledge base, runs the validators that
match the workspace's languages, and exits nonzero when a required intent is
not held. It never asks a model, never reads the clock, and never executes
project code: given the same workspace, manifest, knowledge base, and
`cmv.toml`, its output is byte-identical.

Run it from the project root, or pass `--root <project>`.

## Commands

```text
cmv check  [--json] [--strict] [--root <project>] [--manifest <path>] [--knowledge-base <path>]
cmv status [--json]            [--root <project>] [--manifest <path>] [--knowledge-base <path>]
```

| Command | Description |
| --- | --- |
| `cmv check` | Run every compiled intent's validators and report one verdict per intent |
| `cmv check --strict` | Also fail the run when any intent could not be checked |
| `cmv check --json` | Emit the full per-intent report as JSON for CI |
| `cmv status` | Summarize the manifest, pin, languages, and validator coverage without running anything |

Defaults:

| Input | Default |
| --- | --- |
| `--root` | the current directory |
| `--manifest` | `<root>/.context-mixer/cmf-manifest.json`, where `cmf install --local --apply` writes it |
| `--knowledge-base` | the `knowledge_base.path` recorded in the manifest |

`--knowledge-base` overrides where the intent records and validators are read
from. A relative path — on the command line or in the manifest — is resolved
against cmv's working directory.

## Language detection

cmv verifies as the languages it finds at the project root (not below it):

| File at the root | Language |
| --- | --- |
| `Cargo.toml` | `rust` |
| `pyproject.toml`, `setup.cfg`, `setup.py`, or any `requirements*.txt` | `python` |
| `package.json` | `typescript` when `tsconfig.json` is beside it, else `javascript` |
| `go.mod` | `go` |

A validator runs only when its `language` is in that set. Setting `languages`
in `cmv.toml` replaces detection entirely.

## Project config: `cmv.toml`

Hand-authored, at the project root, entirely optional. It carries what neither
the knowledge base nor the code can supply.

```toml
# Replace detection; an empty list runs no validators.
languages = ["rust"]

# Seconds before a validator is killed and its intent reported unchecked.
# Default 60.
validator_timeout_seconds = 30

# Per-intent settings, keyed by catalog key. Each table is handed to that
# intent's validators verbatim as the --config JSON document; an intent
# without a table receives {}.
[intent."craftsperson/rust/isolate-functional-core"]
business_rule_pattern = "\\b10_?000\\b|\\b500\\b"
business_rule_minimum_matches = 2

[intent."craftsperson/python/nonblocking-async-io"]
blocking_symbols = ["time.sleep", "requests.get"]
```

Unknown top-level keys are rejected, so a misspelled `languages` cannot
silently fall back to detection.

## How validators run

For each intent in the manifest, cmv resolves the record by its `id` first and
its catalog `key` second, so a record that moved in the knowledge base is still
found. It then runs each `static-check` validator whose language matches:

```text
<knowledge-base-root>/<run> --workspace <project-root> --config <tempfile.json>
```

with the knowledge-base root as the working directory. The validator writes one
JSON document to stdout and exits 0:

```json
{
  "applicable": true,
  "followed": false,
  "signals": { "effectful_modules": ["src/http.rs"] },
  "evidence": ["no gateway trait is declared"],
  "locations": [{ "path": "src/http.rs", "line": 14 }]
}
```

`applicable` and `followed` are required; `signals` (default `{}`), `evidence`
(default `[]`), and `locations` (default `[]`; `line` optional) may be omitted.

When several validators match one intent (a record with one validator per
language, in a workspace with several languages) they combine
**all-must-pass**: any failure fails the intent; otherwise any unchecked run
leaves it unchecked; otherwise it passes when at least one run was applicable.

## Verdict states

| State | Meaning | Effect on exit code |
| --- | --- | --- |
| `pass` | Validator ran; `applicable: true`, `followed: true` | — |
| `fail` | Validator ran; `applicable: true`, `followed: false` | `1` if the validator is `required` |
| `not_applicable` | Validator ran; `applicable: false` — the condition never arose | — |
| `unchecked` | No validator for the workspace's languages, record missing from the knowledge base, validator could not start, crashed, timed out, or wrote no parseable verdict — the reason is reported | `1` only with `--strict` |
| `unguided` | The manifest lists the intent as dropped; the guidance never reached the artifact | — |

An intent is additionally marked **stale** when its record's bytes no longer
match the checksum the manifest recorded at compile time. Stale is
informational and never changes the exit code; the remedy is to re-run
`cmf install`, because a newer knowledge base may change *selection*, which is
cmf's decision.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Every required, applicable validator passed |
| `1` | A required validator failed, or, with `--strict`, an intent was unchecked |
| `2` | Missing or malformed manifest, unreadable knowledge base, or bad usage |

An optional validator's failure is reported but never changes the exit code.

## Human output

A linter-style listing, one line per intent, grouped by state with settled
outcomes first and problems last, so they sit beside the summary:

```text
PASS       craftsperson/rust/put-gateways-at-effect-boundaries
           every effectful call crosses a gateway trait
UNGUIDED   craftsperson/rust/name-for-intent
           budget
UNCHECKED  craftsperson/rust/compile-public-documentation
           no validator for languages [rust]
FAIL       craftsperson/rust/isolate-functional-core  (required)  (stale: record changed since compile)
           business rules live beside I/O in src/main.rs
           src/main.rs:12

1 pass, 1 fail, 0 not applicable, 1 unchecked, 1 unguided; adherence 50.0%
1 record changed in the knowledge base since compile; re-run `cmf install` to recompile.
```

Under each intent come its evidence strings, then its locations as
`path:line`, then the reason when unchecked or unguided. The adherence rate is
`pass / (pass + fail)`: the denominator is what the code had occasion to
exhibit.

## JSON report

`cmv check --json` writes one pretty-printed document. It carries no timestamp
and no temporary path, so it is byte-stable across runs.

```json
{
  "schema": 1,
  "manifest": {
    "profile": { "id": "rust-shipping", "version": "0.3.0" },
    "knowledge_base": { "source": "guidelines", "path": "kb", "revision": "a1b2c3d4…" }
  },
  "languages": ["rust"],
  "intents": [
    {
      "id": "guidelines.intent.isolate-functional-core",
      "key": "craftsperson/rust/isolate-functional-core",
      "language": "rust",
      "required": true,
      "state": "fail",
      "description": "No module holding business rules references a gateway.",
      "signals": { "business_rule_minimum_matches": 2 },
      "evidence": ["business rules live beside I/O in src/main.rs"],
      "locations": [{ "path": "src/main.rs", "line": 12 }],
      "stale": true
    }
  ],
  "summary": {
    "pass": 1,
    "fail": 1,
    "not_applicable": 0,
    "unchecked": 1,
    "unguided": 1,
    "adherence_rate": 0.5,
    "exit_code": 1
  }
}
```

- `manifest` echoes the manifest's `profile` and `knowledge_base` as recorded
  at compile time (`source` and `revision` are omitted when the manifest has
  none).
- `intents` lists every compiled intent in manifest order, then every dropped
  intent. `state` is one of `pass`, `fail`, `not_applicable`, `unchecked`,
  `unguided`; `reason` is present for the last two. `language` and
  `description` are `null` when no validator ran; `id` is `null` for a dropped
  intent whose record is no longer in the knowledge base. `language` and
  `description` join several validators' values with `, ` and ` / `
  respectively, and `signals` is then an object keyed by language.
- `summary.adherence_rate` is `pass / (pass + fail)` rounded to four decimals,
  or `null` when nothing was applicable. `summary.exit_code` is the code cmv
  exits with.

`cmv status --json` emits `schema`, `manifest_path`, `profile`, `artifact`,
`knowledge_base` (`path`, `exists`, `source`, `revision`), `languages`,
`intents`, `dropped`, and `coverage` (`with_validator`, `missing`, `stale`),
with `coverage` `null` when the knowledge base could not be scanned.

## What is not here yet

cmv reads the knowledge base from the path the manifest recorded or from
`--knowledge-base`. Resolving it through the cmx source registry at the pinned
revision, `cmv explain <intent-key>`, and release packaging follow in later
commits; see `CMV.md`.
