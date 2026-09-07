# cmv — Deterministic Verification of Compiled Intents

> Design draft. Status: proposed (2026-09-05). Phases 1–4 are implemented
> (cmv check, status, explain; source-registry resolution; pinned revision;
> Rust validators live in the guidelines intent atlas with calibration
> fixtures; the `intent-atlas` crate is extracted). Phases 5–6 and 8
> (sensors) pending. Companion to [CHARTER.md](CHARTER.md),
> [SPEC.md](SPEC.md), and [SETS.md](SETS.md).

## Motivation

cmf composes a set of intent records relevant to a repository and materializes
them as guidance. Whether the repository's code then *holds those intents true*
is a separate question, and today only the behavioural exercises under
[benchmark/exercises](benchmark/exercises/README.md) answer it — by parsing the
finished code with per-intent deterministic checks, never by asking a model.

Those checks proved three things worth keeping:

- **Parsing beats asking.** A verdict derived from the code is reproducible,
  cheap, and cannot be talked into agreement. What an agent said it would do is
  not evidence that it did.
- **The question forms generalize; the substrate does not.** Sixteen Python
  checks and eight Rust checks share one verdict shape, one config split, and
  one three-state result, but Rust needed its own fact extractor because no
  Python parser reaches it. Each language is best at reading its own source.
- **Checks get corrected, often.** Seven scoring corrections so far. One of
  them produced a false "the guidance breaks the software" headline until the
  archived trials were re-scored.

But the checks live in the benchmark, keyed to intents by a Python dictionary
that drifts from the intent atlas silently, and nothing outside the benchmark
can run them. A project that installed cmf guidance has no way to ask whether
its code still follows it.

**cmv** makes that question a normal part of quality verification. It runs like
a linter or a test runner from a project root, reads the manifest cmf compiled
for that project, executes each intent's codified validator, and exits nonzero
when a required intent is not held.

## Three responsibilities

The **intent atlas** is the knowledge base: the ecosystems it supports, each
with the sensors that recognize it in a project; the intent records in and
around those ecosystems, placed in the directory hierarchy that names their
ecosystem; the validators beside the intents; and the profiles that name a
slice of it for a delivery surface. The guidelines repository's site of the
same name is one view of it; the library cmf and cmv share is another reader
of the same shape. Everything below that says *atlas* means this.

| | Owns | Reads | Writes |
| --- | --- | --- | --- |
| **Intent atlas** | Ecosystems and their sensors; intent records and their validators; profiles | — | Sensors, records, validators, calibration fixtures |
| **cmf** (composes) | Selection and rendering | Intent atlas, profile | Guidance artifact, **manifest** |
| **cmv** (verifies) | Dispatch, aggregation, exit code | Manifest, project config, workspace | Nothing |

The intent atlas stays outside cmx/cmf/cmv, as the charter already requires.
cmf's output grows from one artifact to two. cmv is read-only.

## Design decisions (locked)

1. **cmv is purely deterministic.** Given the same workspace, manifest, and
   project config, its output is byte-identical. It has no `llm` feature, never
   falls back to a model when a validator is missing, reads nothing outside its
   inputs, and executes no project code. This holds by construction, not by
   convention.
2. **The manifest is cmf's second output artifact.** It is not a debugging aid
   or a side effect. It has a schema, a version field, and a conformance
   fixture, because cmv depends on it and the two tools release on different
   cadences.
3. **Validators live in the intent atlas, beside their records.** A
   validator is declared as an evidence entry on the intent record. cmv carries
   no opinion about any intent; every judgement is in the validator, so cmv
   stays stable while validators keep being corrected.
4. **One validator per language per intent.** A record may declare several
   validators, each naming the language it reads. cmv runs the ones matching
   the workspace. A language with no validator for an intent reports
   *unchecked*, never *pass*.
5. **The intent atlas is the registry, pinned by revision.** cmv resolves
   validators from the intent atlas at the revision the manifest records.
   The intent atlas is a git repository, so it is a cmx source; nothing new
   is hosted. Vendoring validator code into projects would strand corrections;
   an unpinned live registry would make CI irreproducible. A pin gives both.
6. **Static analysis first.** Every one of the twenty-four existing checks is
   static, including the ones that sound like runtime properties (nonblocking
   async I/O, timeouts, resource cleanup). The runtime tier starts empty and
   arrives later through the project's own test runner (see
   [Runtime evidence](#runtime-evidence-later)).
7. **`required` governs gating; `status` is informational.** An intent
   record's maturity (`hypothesized`, `confirmed`, …) is shown in output but
   does not change the exit code. Only the `required` flag on the validator's
   evidence entry does.

## Data model

### Intent record: validator evidence entries

Records already carry an `evidence` list whose entries have a `type`, a
`description`, and a `required` flag. Today the only type in use is
`architecture_review`, a human procedure. Validators are a new evidence type:

```toml
evidence = [
  { type = "architecture_review", description = "Core modules depend on project-owned gateway contracts rather than concrete vendor clients.", required = true },
  { type = "static-check", language = "rust", run = "checks/rust/put_gateways_at_effect_boundaries.py", description = "No effectful call is made outside a gateway module.", required = true },
  { type = "static-check", language = "python", run = "checks/python/put_gateways_at_effect_boundaries.py", description = "No effectful call is made outside a gateway module.", required = true },
]
```

- `description` — required on every entry, validator or not. It is what the
  agent sees: cmf renders it into the guidance under *Require* or *Observe
  when useful*, so the agent knows what will be checked. `language` and `run`
  never reach the artifact.
- `language` — the source language this validator reads. Matched against the
  workspace's detected languages.
- `run` — path relative to the atlas root. Any executable; the
  language of the validator's implementation is independent of the language
  it validates, though in practice each language's own toolchain parses it
  best. The validator owns its fact extraction (Python's `ast`, the `syn`
  binary for Rust, …). `language` and `run` together are what make a
  `static-check` entry a validator; a `static-check` entry with neither is
  descriptive evidence — "a static check should verify X" — that declares
  nothing executable.
- `required` — whether a `fail` verdict fails the cmv run.

cmf validates these at scan time. A `static-check` entry with neither `run`
nor `language` is ordinary non-executable evidence: the intent atlas already
uses the type descriptively (an expectation that a static check should verify
something), and that is exactly the kind of expectation a validator later makes
executable. cmf refuses the intent atlas, naming the record and the entry,
when a `static-check` entry carries exactly one of the two fields
(half-declared — both are needed to declare a validator), when either field
appears on an entry of any other type, or when `run` is absolute or contains a
`..` component — it resolves against the atlas root and must stay
inside it.

`cmf`'s catalog schema (`Evidence` in
[cmf/src/catalog.rs](cmf/src/catalog.rs)) carries the optional `language` and
`run` fields. Entries without both remain non-executable evidence;
`Evidence::validator()` returns `None` for them.

### Records have two identifiers; the manifest records both

- **key** — path-derived below `intents/`, without `.toml`, e.g.
  `craftsperson/rust/put-gateways-at-effect-boundaries`. This is how the
  catalog is addressed and how the profile selects.
- **id** — the `id` field inside the record, e.g.
  `guidelines.intent.put-gateways-at-effect-boundaries`. Stable across file
  moves.

cmv resolves by `key` first. The key is the compile-time locator — it is what
cmf selected and what the artifact was assembled from — and the manifest
checksum already flags a record whose content changed. An `id` alone cannot
locate a record, because ids recur across collections by design: the atlas
specializes one intent per language (nine records carry
`guidelines.intent.isolate-functional-core-from-effects`, one per craftsperson
collection), and resolving by id would silently pick a sibling's record.

Only when the key is absent from the catalog does cmv fall back to the `id`,
and only when exactly one record carries it — a record that moved. When the key
is absent and several records share the id, the intent is not found; the reason
names the records sharing the id, and the remedy is `cmf install` to recompile
against the current keys.

### Records carry their ecosystem in the key; the manifest records the profile's

The intent atlas states that its directory hierarchy is the realization
hierarchy: `craftsperson/python/uv/` specializes Python guidance for uv-based
projects. A record's **ecosystem qualifiers** are therefore the key segments
between the collection root and the slug — `["python", "uv"]` for
`craftsperson/python/uv/pin-interpreter`, `["rust"]` for
`craftsperson/rust/use-structured-tracing`, none for
`craftsperson/isolate-functional-core-from-effects`. No record field restates
this; `cmf::catalog::ecosystem_qualifiers` derives it from the key.

A profile declares the ecosystems it targets in `[select] ecosystems`, and cmf
admits a record only when every qualifier it has is declared (a record with no
qualifiers is always eligible; a profile declaring nothing filters nothing).
Category/tag selection drops ineligible records, graph expansion skips edges to
them, and an explicit key outside the declared ecosystems is a profile error.

The declared list also makes it safe to walk the `specializes` edges downward.
When a profile declares ecosystems and `prefer_specializations` holds, cmf pulls
in every eligible record that specializes a selected one, transitively (a
general record pulls `python/…`, which pulls `python/uv/…` when `uv` is
declared), and then drops the shadowed general parent — so a profile that
selected general advice by category and tag compiles the ecosystem's own
version of it. The manifest's `intents` list therefore names the specialized
records, not the general one, and cmv verifies those. Without a declared list
the walk is a no-op, since it would otherwise pull every language's version.

The manifest records the declared list as `profile.ecosystems` — always
present, empty when the profile declared none — beside the profile `id` and
`version`. cmv does not yet act on it; the intended use is to compare it with
the languages cmv detects in the workspace and report a profile compiled for
an ecosystem the project does not contain.

### The manifest

Written by `cmf install`, committed with the project, read by cmv. JSON,
because it is machine-written state, matching `cmx-lock.json` and `sets.json`
and reusing cmx-core's `json_file` helpers. It lives in the project-local cmx
state directory beside the local lock file.

```json
{
  "schema": 1,
  "compiled_at": "2026-09-05T14:02:11Z",
  "atlas": {
    "source": "guidelines",
    "revision": "a1b2c3d4e5f6"
  },
  "profile": {
    "id": "rust-craftsperson-shipping",
    "version": "0.1.0",
    "ecosystems": ["rust"]
  },
  "artifact": {
    "name": "AGENTS",
    "surface": "agent",
    "checksum": "sha256:…"
  },
  "intents": [
    {
      "id": "guidelines.intent.put-gateways-at-effect-boundaries",
      "key": "craftsperson/rust/put-gateways-at-effect-boundaries",
      "checksum": "sha256:…"
    }
  ],
  "dropped": [
    {
      "key": "craftsperson/rust/compile-public-documentation",
      "reason": "budget"
    }
  ]
}
```

- `atlas.source` is a cmx source name; `revision` is the git commit
  cmf read. For a local (non-git) source, `revision` is omitted and cmv
  reports the pin as unavailable.
- `intents` is exactly `Assembly.selected` — the keys retained in the
  delivered artifact after budget enforcement — plus each record's content
  checksum, so cmv can tell a record changed even when the revision is
  unavailable.
- `dropped` records what the profile asked for that did not survive
  assembly, with the reason. cmv reports these as *unguided* and never fails
  them: the guidance never arrived, so failing them would measure the budget,
  not the code.

The manifest is the `--explain` output made durable and given a contract.
The block was named `knowledge_base` when Phase 1 shipped; it is renamed
`atlas` with the crate extraction (Phase 7). Nothing has been released, so the
schema number stays at 1.

Phase 1 shipped the manifest as specified above, with three concrete details.
It lives at `.context-mixer/cmf-manifest.json`, written by
`cmf install --local --apply` (the preview says it will be; global installs
write none, per the open decision below), and `cmf assemble --manifest <path>`
writes the same document wherever asked. `atlas` gained a `path`
field — the root as cmf resolved it — so the record is still locatable when
`source` is absent because the root is not a registered cmx source. And
`dropped` is present but always empty for now: assembly fails on budget
overrun rather than dropping, so nothing has yet had a reason to appear there.
The field stays so the schema does not move when assembly learns to drop.

### Project config

Some knowledge is neither in the intent atlas nor derivable from the code:
which literals mark a project's business rules, which symbols count as blocking
in its domain, what its package is called. The benchmark holds this per
scenario as `check_config`; a project holds it in a hand-authored `cmv.toml`
at its root, the way `clippy.toml` or `ruff.toml` do:

```toml
[intent."craftsperson/rust/isolate-functional-core-from-effects"]
business_rule_pattern = "\\b10_?000\\b|\\b500\\b|\\b20\\b"
business_rule_minimum_matches = 2

[intent."craftsperson/python/nonblocking-async-io"]
blocking_symbols = ["time.sleep", "requests.get"]
```

Convention: hand-authored inputs are TOML at the project root; machine-written
state is JSON in the state directory.

## Validator invocation protocol

cmv runs each selected validator as a subprocess:

```text
<atlas-root>/<run> --workspace <project-root> --config <tempfile.json>
```

- `--config` points at a JSON document holding the intent's block from
  `cmv.toml` (empty object when absent).
- The validator writes one JSON document to stdout and exits 0:

```json
{
  "applicable": true,
  "followed": false,
  "signals": { "declared_traits": {}, "effectful_modules": ["src/http.rs"] },
  "evidence": ["no gateway trait is declared"],
  "locations": [{ "path": "src/http.rs", "line": 14 }]
}
```

This is the shape every existing check already returns
(`result` / `not_applicable` in
[benchmark/exercises/predicates.py](benchmark/exercises/predicates.py)), with
one addition: `locations`, so cmv can render diagnostics with file and line
the way a linter does. The Rust fact extractor emits line numbers and Python's
`ast` has them; existing checks need to start surfacing them.

- A nonzero exit, a timeout, or unparseable stdout makes the intent
  *unchecked* with the reason captured. cmv never guesses.
- Validators may not read the clock, the network, or anything outside the
  workspace and their config. cmv cannot enforce this, so the intent atlas's
  own gate must (see [Calibration fixtures](#calibration-fixtures)).

### Ecosystem detection

cmv runs every validator whose `language` is among the ecosystems detected in
the workspace. Detection is the atlas's sensors (next section), evaluated
against the project root; the same detected set feeds cmf's mismatch warning.
It is overridable in `cmv.toml` (`ecosystems = ["rust"]`). Until Phase 8
lands, cmv detects from a built-in table of root build files (`Cargo.toml` →
rust, `pyproject.toml` → python, …), which the sensors replace.

## Ecosystems and sensors

The atlas knows which ecosystems it supports; the tooling does not. A sensor
file at the atlas root, `ecosystems.toml`, declares each ecosystem and how to
recognize it in a project:

```toml
[python]
signatures = [
  { file = "pyproject.toml" },
  { file = "setup.py" },
  { glob = "requirements*.txt" },
]

[uv]
implies = ["python"]
signatures = [
  { file = "uv.lock" },
  { file = "pyproject.toml", contains = "[tool.uv]" },
]

[phoenix]
implies = ["elixir"]
signatures = [{ file = "mix.exs", contains = ":phoenix" }]
```

- An ecosystem is **detected** when any of its signatures holds. Its
  `implies` list is then detected too, transitively.
- **Predicate kinds** are the extension point: `file` (a root-relative path
  exists), `glob` (a root-relative pattern matches at least one file), and
  `contains` (combined with `file`: the file holds the literal). New kinds —
  a dependency named in a manifest, a regex — are added to the matcher without
  touching existing signatures. A sensor file that uses a kind the tooling
  does not know is a scan error naming it, never a silent miss.
- Every predicate is evaluated through the filesystem gateway and reads only
  the project root, so the matcher is pure and fixture-testable, and cmv stays
  deterministic: at a pinned revision, the sensors are pinned with everything
  else.
- The names are the atlas's own vocabulary: an ecosystem's name must be a
  directory the realization hierarchy uses (`python`, `rust`, `python/uv` →
  `uv`), so a sensor's `implies` must agree with the nesting. cmf reports a
  disagreement as an atlas error at scan time.
- An atlas with no sensor file detects nothing. cmf then cannot warn about a
  profile's ecosystems and cmv reports every validator-bearing intent
  *unchecked* with the reason, rather than guessing from a built-in table.

What the detected set is for:

- **cmf**, at `install --local`: compare the profile's declared ecosystems
  with the detected set and warn — a profile compiled for `rust` against a
  Python project is almost certainly the wrong profile. A profile declaring no
  ecosystems gets no warning, since it filters nothing.
- **cmv**: choose which validators run (an entry's `language` must be in the
  detected set), and flag a manifest whose `profile.ecosystems` names an
  ecosystem the workspace does not contain. Both informational; neither
  changes the exit code.

## Verdict states and exit codes

Per intent, cmv reports exactly one of:

| State | Meaning | Effect on exit |
| --- | --- | --- |
| **pass** | Validator ran, `applicable: true`, `followed: true` | — |
| **fail** | Validator ran, `applicable: true`, `followed: false` | Nonzero if `required` |
| **not applicable** | Validator ran, `applicable: false` — the condition never arose | — |
| **unchecked** | No validator for this language, extractor missing, crash, timeout | Nonzero only with `--strict` |
| **unguided** | In `dropped`; the guidance never reached the artifact | — |

Exit codes: `0` when every required, applicable validator passed; `1` when any
required validator failed (or, under `--strict`, any was unchecked); `2` for a
missing or malformed manifest, unresolvable intent atlas, or bad usage.

The adherence rate, when shown, is `pass / (pass + fail)` — the denominator is
what the code had occasion to exhibit, exactly as the benchmark computes it.

When the intent atlas's `HEAD` has moved past the pinned revision, validators
run from the **pinned tree** (materialized with `git archive` into the run's
scratch directory), so verdicts come from the intent atlas as compiled.
Additionally, cmv reports **stale** per intent by comparing against `HEAD`, not
the pinned tree: the record's bytes at `HEAD` differ from the manifest
checksum, or any of its validators' `run` files differ between `HEAD` and the
pinned tree. Every report also says whether the intent atlas has `moved`
(`HEAD` differs from the pin) and which tree was `verified_against`. All of
this is informational and does not change the exit code; the remedy is to
re-run `cmf install`, because a newer intent atlas may change *selection*,
which is cmf's decision, not cmv's. cmv never re-pins.

## Command surface

```text
cmv check [--json] [--strict] [--manifest <path>] [--root <project>]
cmv explain <intent> [--json] # show the validator(s) that would run and why
cmv status [--json]           # manifest summary, pin, stale/unavailable, languages detected
```

Human output is a linter-style listing grouped by state with `path:line`
diagnostics from `locations`; `--json` emits the full per-intent report for CI
and for the benchmark aggregator.

`explain` takes a catalog key, or a record `id` when the argument contains a
`.`, and runs nothing. It reports how the manifest entry resolved (by id, by
key, not found), the record's title and status, every declared validator with
its language, `run`, `required` flag, and description, which would run for the
detected languages and with exactly which argv, the `--config` document from
`cmv.toml`, the stale flag, and whether the manifest dropped the intent. It
exits `2` when the intent is not in the manifest at all.

All three commands accept `--atlas <path>` as an override of
source-registry resolution (the order is override, then the manifest's
`source` in the cmx registry, then the manifest's recorded `path`; the report
records which answered). The override stays useful for a checkout that is not
a registered cmx source (a validator author's working copy, a CI cache). They
also accept `--at-head`, which verifies the intent atlas's working tree even
when the manifest pins a revision its `HEAD` has moved past; the report says
so. This is for validator authors iterating on an intent atlas.

## Calibration fixtures

A validator that cannot fail measures nothing. The benchmark enforces this per
scenario — the reference solution must score full marks and the untouched
skeleton must score zero. The same rule travels into the intent atlas: every
validator ships with at least one workspace fixture it passes and one it fails,
in the language it validates, and the intent atlas's own quality gate runs
them. This is also where the determinism rule is enforced: run each fixture
twice and diff the output.

The rate-card scenario's `reference/` and `input/skeleton/` become the first
Rust fixtures.

## What happens to the benchmark

The benchmark becomes one more caller of cmv rather than the owner of the
checks:

- `checks.py`, `predicates.py`, and `rustfacts/` migrate to the intent atlas
  as validators. `adherence.py`'s `CHECKS` dictionary disappears; the mapping
  is the evidence entries.
- `runner.py` stages the workspace, runs `cmf install --local` (which writes
  the manifest), and after the agent finishes runs `cmv check --json`. The
  hidden acceptance suite is unchanged.
- `rescore.py` becomes `cmv check` over each unpacked archive.
- `aggregate.py` is unchanged; the per-intent JSON it reads has the same
  shape.
- Each `metrics.json` records the atlas revision the validators ran
  at, so a later correction can be distinguished from the evidence it
  re-scored.

## Runtime evidence (later)

Some intents may need observation, not shape: that a timeout actually fires,
that a resource is actually released. cmv never executes the project. Instead,
a plugin for the project's test framework records observations to a file
during the normal test run, and a second evidence type,
`runtime-observation`, tells cmv which file and what to look for. cmv reads it
after the suite has run and stays deterministic over its inputs.

Routing runtime evidence through the test suite inherits the project's own
determinism standard — a flaky test already fails the gate — instead of cmv
inventing timeout and isolation policy, which is the trap the benchmark runner
fell into. An intent may carry both a static validator and a runtime
observation; both must hold.

Not designed further here.

## The `intent-atlas` crate

cmf and cmv both need to know how to work with the atlas, and neither should
depend on the other for it. One library reads the atlas's shape: catalog
scanning and the ecosystem qualifiers a key carries, profile loading and
selection (including eligibility and downward expansion), sensor evaluation,
validator resolution for a record, and the manifest types. It is a workspace
member named `intent-atlas`, beside `cmx-core`; cmf and cmv depend on it, cmv
stops depending on cmf, and cmx never sees it.

It is not part of `cmx-core`, decided: cmx has no use for it, and cmx-core is
twinned with a TypeScript port under a conformance suite and released to
crates.io and npm in lockstep, so anything placed there pays a port and a
coordinated release for a consumer that does not exist. `intent-atlas` is not
published until a second consumer appears. Its own tests are the golden
fixtures cmf and cmv already carry, which must stay byte-identical across the
extraction apart from the renamed manifest block.

## Charter change

CHARTER.md describes "two complementary CLIs". It becomes three:

- **cmx** — the consumer tool: installs, versions, updates, and reconciles
  agents and skills across platforms.
- **cmf** — the materializer: composes a profile-specific set of intents into
  an installed agent or skill, and records what it composed.
- **cmv** — the verifier: checks, deterministically, that a repository holds
  the intents cmf compiled for it.

Goals gain "Verify, from source alone, that a repository holds the intents its
guidance was compiled from." Non-goals gain "Judging adherence with a model.
cmv parses; it never asks." The existing non-goal that a separate tool owns the
intent atlas is unchanged and now also covers validators.

## Phased implementation plan

1. **Manifest.** Schema, `cmf install` writes it, `Assembly.selected` becomes
   a public contract, conformance fixture added. No cmv yet.
2. **Validator evidence entries.** Catalog schema gains `language` and `run`,
   validated at scan time. The invocation protocol and verdict JSON, including
   `locations`, are specified in this note's "Validator invocation protocol"
   section; they become a conformance fixture when cmv lands in Phase 3.
3. **cmv binary.** Done. New workspace member inheriting the workspace
   version. Reads manifest and `cmv.toml`, detects languages, resolves the
   intent atlas through the cmx source registry at the pinned revision
   (materializing the pinned tree when the checkout has moved past it),
   dispatches validators, explains one intent on request, renders human and
   JSON output, exits per the table above. Architecture map, release workflow,
   Homebrew formula, charter, and book coverage updated in the same commits.
4. **Rust validators migrate.** Done. The eight rate-card checks move to the
   intent atlas with `reference/` and `skeleton/` as their fixtures. cmv on
   the reference scores full marks; on the skeleton, zero. This is the first
   end-to-end proof.
5. **Python validators migrate.** Sixteen checks, two scenarios.
6. **Benchmark switches over.** `runner.py` calls cmf and cmv; `rescore.py`
   reduces to cmv over archives; `adherence.py` retired.
7. **The `intent-atlas` crate.** Done. A pure move: catalog, profile, selection,
   and manifest types leave `cmf/src/` for the new workspace member; cmv's
   dependency on cmf is replaced; the manifest's `knowledge_base` block and
   cmv's `--knowledge-base` flag are renamed `atlas` (the old flag stays as a
   hidden alias for one release). No behaviour change; the golden fixtures
   prove it.
8. **Sensors.** `ecosystems.toml` at the atlas root, the matcher with its
   predicate kinds, cmf's mismatch warning at `install --local`, and cmv
   switching from its built-in language table to the atlas's sensors, with the
   guidelines repository declaring its ecosystems in the same change.
9. **Runtime tier** — designed separately once the static tier has run in at
   least one real project's CI.

## Open decisions (not blocking Phase 1)

- **Shared crate placement.** Resolved: the `intent-atlas` workspace member
  (see [The `intent-atlas` crate](#the-intent-atlas-crate)), not `cmx-core`.
- **Manifest location for global installs.** A project-local manifest is
  clear. For a user-wide `cmf install` there is no single repository to
  verify; cmv likely only supports local scope and says so.
- **Validator execution sandbox.** cmv trusts validators from a pinned
  atlas revision the user registered. Whether to restrict filesystem
  or network access beyond documentation is deferred.
- **Multiple validators per language per intent.** Allowed by the schema
  (several entries). Whether they combine as all-must-pass or any-passes is
  undecided; all-must-pass is the conservative default.
