# cmv Command Reference

cmv verifies, deterministically, that a project holds the intents cmf compiled
for it. It reads the compile manifest `cmf install --local` wrote, resolves
each compiled intent's record in the atlas, runs the validators that
match the workspace's languages, and exits nonzero when a required intent is
not held. It never asks a model, never reads the clock, and never executes
project code: given the same workspace, manifest, atlas, and
`cmv.toml`, its output is byte-identical.

Run it from the project root, or pass `--root <project>`.

## Commands

```text
cmv check            [--json] [--strict] [--root <project>] [--manifest <path>] [--atlas <path>] [--at-head]
cmv status           [--json]            [--root <project>] [--manifest <path>] [--atlas <path>] [--at-head]
cmv explain <intent> [--json]            [--root <project>] [--manifest <path>] [--atlas <path>] [--at-head]
```

| Command | Description |
| --- | --- |
| `cmv check` | Run every compiled intent's validators and report one verdict per intent |
| `cmv check --strict` | Also fail the run when any intent could not be checked |
| `cmv check --json` | Emit the full per-intent report as JSON for CI |
| `cmv status` | Summarize the manifest, pin, languages, and validator coverage without running any validator |
| `cmv explain <intent>` | Show what `check` would do for one intent — record resolution, validators, argv, config, stale — without running anything |
| `--at-head` | Verify the atlas's working tree even when the manifest pins a revision its `HEAD` has moved past |

Defaults:

| Input | Default |
| --- | --- |
| `--root` | the current directory |
| `--manifest` | `<root>/.context-mixer/cmf-manifest.json`, where `cmf install --local --apply` writes it |
| `--atlas` | resolved through the cmx source registry, then the manifest (see below); `--knowledge-base` still parses as a hidden alias |

A relative path — on the command line, in the registry, or in the manifest — is
resolved against cmv's working directory.

## Where the atlas comes from

The atlas is the registry (`CMV.md`, design decision 5): it is a git
repository, so it is a cmx source, and cmv finds it the way cmx would. The
resolution order is fixed, and every report says which step answered in its
`atlas.resolved_by` field:

| `resolved_by` | Step |
| --- | --- |
| `override` | `--atlas <path>` (or its hidden alias `--knowledge-base`) was given; nothing else is consulted |
| `source` | the manifest's `atlas.source` names a registered cmx source (`cmx source list`); its local directory or clone is used |
| `path` | the manifest's recorded `atlas.path` |

A recorded source name the registry does not know falls through to `path` with
a warning on stderr naming the source and suggesting `cmx source add`: the
manifest still says where the records were when cmf read them, so the run can
proceed.

## The pinned revision

The manifest records the git `revision` cmf read. When the resolved atlas is
a git checkout whose `HEAD` differs from that pin, cmv verifies against
the **pinned tree**, not the working tree: it runs

```text
git -C <atlas> archive --format=tar -o <scratch>/kb.tar <revision>
tar -xf <scratch>/kb.tar -C <scratch>/kb
```

and reads records from, and runs validators in, `<scratch>/kb` (executable bits
survive `git archive`). Validators' `run` paths resolve inside it. The scratch
directory is removed when the run ends and never appears in output.

When `HEAD` equals the pin, the manifest has no `revision`, or the root is not
a git checkout, cmv uses the root directly. When the pinned revision cannot be
archived — typically because the checkout has not fetched it — cmv exits `2`
naming the revision and suggesting `cmx source update <name>` (or `git fetch`
when the atlas is not a registered source).

`--at-head` bypasses the pin and verifies the working tree; the report's
`verified_against` says `head`. This is for validator authors iterating on an
atlas: edit the script, run `cmv check --at-head` against a project,
repeat. It never re-pins anything.

Every `check`, `status`, and `explain` report carries an `atlas` block:

| Field | Meaning |
| --- | --- |
| `path` | the atlas root as resolved |
| `resolved_by` | `override`, `source`, or `path` |
| `source` | the cmx source name the manifest recorded, if any |
| `pinned_revision` | the manifest's `revision`, if any |
| `head_revision` | the working tree's `HEAD`, when it is a git checkout |
| `verified_against` | `pinned` when the verdicts came from the pinned revision (materialized, or checked out as `HEAD`); `head` otherwise |
| `moved` | `true` when `HEAD` differs from the pin |

`moved` is informational and never changes the exit code. In human output it
adds one line after the summary:

```text
atlas has moved: HEAD ffffffffffff vs pinned a1b2c3d4e5f6; 2 compiled records or validators changed; re-run cmf install to recompile
```

Recompiling is cmf's decision — a newer atlas may change *selection* —
so cmv only reports it.

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
the atlas nor the code can supply.

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

For each intent in the manifest, cmv resolves the record by its catalog `key`
first: the key is the compile-time locator, and a record's `id` may
intentionally recur across collections (one specialization per language), so
an id on its own cannot locate a record. When no record sits at the key, cmv
falls back to the `id` only if exactly one record carries it — a record that
moved. When several do, the intent is reported unchecked with a reason naming
those records; `cmf install` recompiles against the current keys. cmv then runs
each `static-check` validator whose language matches:

```text
<atlas-root>/<run> --workspace <project-root> --config <tempfile.json>
```

with the atlas root as the working directory. The validator writes one
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
| `unchecked` | No validator for the workspace's languages, record missing from the atlas, validator could not start, crashed, timed out, or wrote no parseable verdict — the reason is reported | `1` only with `--strict` |
| `unguided` | The manifest lists the intent as dropped; the guidance never reached the artifact | — |

An intent is additionally marked **stale** when, at the working tree's `HEAD`,
its record's bytes no longer match the checksum the manifest recorded at
compile time, or — when validators ran from a materialized pinned tree — any
of its validators' `run` files differ between `HEAD` and that tree. Stale is
always computed against the working tree, never the pinned tree, so a moved
atlas shows exactly which compiled intents it touched. Stale is
informational and never changes the exit code; the remedy is to re-run
`cmf install`, because a newer atlas may change *selection*, which is
cmf's decision.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Every required, applicable validator passed |
| `1` | A required validator failed, or, with `--strict`, an intent was unchecked |
| `2` | Missing or malformed manifest, unreadable atlas, or bad usage |

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
1 record changed in the atlas since compile; re-run `cmf install` to recompile.
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
    "atlas": { "source": "guidelines", "path": "kb", "revision": "a1b2c3d4…" }
  },
  "atlas": {
    "path": "/home/me/guidelines",
    "resolved_by": "source",
    "source": "guidelines",
    "pinned_revision": "a1b2c3d4…",
    "head_revision": "ffffffff…",
    "verified_against": "pinned",
    "moved": true
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

- `manifest` echoes the manifest's `profile` and `atlas` as recorded
  at compile time (`source` and `revision` are omitted when the manifest has
  none).
- `atlas` is where cmv actually read from and which tree it verified;
  see "The pinned revision" above for every field.
- `intents` lists every compiled intent in manifest order, then every dropped
  intent. `state` is one of `pass`, `fail`, `not_applicable`, `unchecked`,
  `unguided`; `reason` is present for the last two. `language` and
  `description` are `null` when no validator ran; `id` is `null` for a dropped
  intent whose record is no longer in the atlas. `language` and
  `description` join several validators' values with `, ` and ` / `
  respectively, and `signals` is then an object keyed by language.
- `summary.adherence_rate` is `pass / (pass + fail)` rounded to four decimals,
  or `null` when nothing was applicable. `summary.exit_code` is the code cmv
  exits with.

`cmv status --json` emits `schema`, `manifest_path`, `profile`, `artifact`,
`atlas` (the block above plus `exists`), `languages`, `intents`,
`dropped`, and `coverage` (`with_validator`, `missing`, `stale`), with
`coverage` `null` when the atlas could not be scanned. The human form
adds `HEAD revision` and `Verified against` lines after `Pinned revision`, and
the `atlas has moved` line at the end when `moved`.

## Explaining one intent

`cmv explain <intent>` answers "what would `check` do with this one?" without
running anything. The argument is a catalog key, or a record `id` when it
contains a `.`. cmv exits `2` when the manifest neither compiled nor dropped
the intent.

```text
Intent: craftsperson/rust/isolate-functional-core
Id: guidelines.intent.isolate-functional-core
Record: resolved by key
Title: Isolate the functional core
Status: confirmed (informational; never gates)
Compiled: yes
Dropped: no
Stale: yes (record or validator changed at HEAD since compile)
Atlas: /home/me/guidelines (resolved by cmx source, verified against pinned revision)
Languages: rust
Config: {"business_rule_minimum_matches":2,"business_rule_pattern":"\\b500\\b"}
Validators:
  rust  checks/rust/isolate_functional_core.py  (required)  would run
    No module holding business rules references a gateway.
    <pinned-tree>/checks/rust/isolate_functional_core.py --workspace /home/me/project --config <scratch>/1.json
  python  checks/python/isolate_functional_core.py  (required)  skipped: language python is not among the workspace's [rust]
    No module holding business rules imports an I/O client.
    <pinned-tree>/checks/python/isolate_functional_core.py --workspace /home/me/project --config <scratch>/1.json
```

- `Record` is `resolved by key`, `resolved by id` (no record at the manifest's
  key, but exactly one carries its id — a moved record), or `not found in
  atlas`, followed in parentheses by the records sharing the id when
  that is what stopped the fallback.
- `Config` is the exact JSON document the validators would receive through
  `--config`: the intent's `[intent."<key>"]` table from `cmv.toml`, or `{}`.
- Each validator line shows its language, `run`, whether it is required, and
  whether it would run here; below it, its description and the exact argv.
  `<scratch>` stands for the per-run temporary directory and `<pinned-tree>`
  for a materialized pinned revision — both are created at check time and never
  appear in output, so `explain` is byte-stable like everything else. When cmv
  verifies the checkout directly, the program path is the real one.
- A dropped intent shows `Dropped: yes (<reason>)`; its validators are listed
  but marked skipped, because the guidance never reached the artifact.

`cmv explain <intent> --json` emits `schema`, `atlas`, `languages`,
and `intent` (`key`, `id`, `resolution`, `title`, `status`, `compiled`,
`dropped`, `drop_reason`, `stale`, `config`, and `validators`, each with
`language`, `run`, `required`, `description`, `would_run`, `skipped`, and
`argv`).
