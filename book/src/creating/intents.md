# Writing Intents

Intent records are the canonical source for guidance. Each TOML file captures
one falsifiable engineering intention: the capability it enables, the threat it
addresses, the expectation behind it, the strategy delivered to an agent, and
the tradeoff incurred by following it.

Generated agents and skills are delivery artifacts. Do not treat them as a
second authoring source.

## Repository structure

Intent records live below `intents/`. Nested directories express collections
and specialization contexts:

```text
intents/
└── craftsperson/
    ├── verify-before-declaring-completion.toml
    └── rust/
        └── require-green-cargo-tests.toml
```

The first record's key is
`craftsperson/verify-before-declaring-completion`; the nested record's key is
`craftsperson/rust/require-green-cargo-tests`.

## Record shape

```toml
id = "guidelines.intent.verify-before-declaring-completion"
title = "Verify before declaring completion"
category = "quality"
tags = ["verification", "completion", "evidence"]
status = "hypothesized"
confidence = 0.99
capability = "Completion claims describe checked behavior."
threat = "A contributor reports success from partial evidence."
expectation = "Local edits can fail tests or integration checks."
strategy = "Review the diff, run proportionate checks, and disclose gaps."
tradeoff = "Verification adds latency."

[[relations]]
type = "specializes"
target = "craftsperson/verify-work"
```

`category` is the primary stable area of concern. Tags provide lateral
classification. Relationships form the semantic graph: `specializes` makes
general guidance concrete for a narrower context, while `related-to` records a
meaningful non-hierarchical association.

## Ecosystems

The directory hierarchy is the **realization hierarchy**: `craftsperson/python/`
specializes the general craftsperson catalogue for Python, and
`craftsperson/python/uv/` narrows that to uv-based projects. A record's
**ecosystem qualifiers** are the key segments between the collection root and
the slug — `["python", "uv"]` for `craftsperson/python/uv/pin-interpreter`,
none for a general record. No record field restates this; the tooling derives
it from the key.

A profile declares the ecosystems it targets, and cmf admits a record only when
every qualifier it has is declared, so a Python profile never compiles Java
advice. With the ecosystems declared, cmf also walks `specializes` edges
downward: general advice selected by category and tag compiles to the
ecosystem's own version of it. See
[Ecosystems](../reference/cmf-commands.md#ecosystems) in the cmf reference.

The atlas declares which ecosystems it supports, and how to recognize each at
a project root, in `ecosystems.toml` beside `intents/`:

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
```

Every name must be a directory the hierarchy uses, and a nested ecosystem must
`implies` its parent; cmf and cmv check this against the records when they
read the atlas. cmf uses the detected set to warn when a profile targets an
ecosystem the project does not show; cmv uses it to choose which validators
run. An atlas with no sensor file detects nothing, and both tools say so rather
than guessing. The predicate kinds and their rules are in
[Sensors](../reference/cmf-commands.md#sensors).

## Validators

A record may declare, as an evidence entry, an executable that decides from a
project's source alone whether the code holds the intent:

```toml
evidence = [
  { type = "architecture_review", description = "Core modules depend on project-owned gateway contracts rather than concrete vendor clients.", required = true },
  { type = "static-check", language = "rust", run = "checks/rust/put-gateways-at-effect-boundaries.py", description = "A project-owned gateway trait is declared, implemented by a concrete adapter, and depended upon by at least one module that performs no I/O.", required = true },
]
```

- `description` is what an agent sees — cmf renders it into the guidance under
  *Require* — so write it as the observable expectation the check enforces, not
  as "runs a script".
- `language` is the source language the validator reads, in the atlas's
  ecosystem vocabulary; cmv runs the entries matching the ecosystems it
  detects.
- `run` is a path relative to the atlas root. Any executable works; the
  validator owns its own fact extraction (Python's `ast`, a `syn` binary for
  Rust).
- `required` decides whether a failing verdict fails the verification run.

A `static-check` entry with neither `language` nor `run` is ordinary descriptive
evidence — an expectation a validator may later make executable. An entry with
only one of the two is rejected when the atlas is read.

cmv runs a validator as `<atlas-root>/<run> --workspace <project> --config
<json>` and expects one JSON document on stdout: `applicable`, `followed`,
`signals`, `evidence`, and `locations`. Anything else — a crash, a timeout,
unparseable output — leaves the intent *unchecked* with the reason; cmv never
guesses. The contract is specified in
[How validators run](../reference/cmv-commands.md#how-validators-run).

**A validator that cannot fail measures nothing.** Every validator ships with a
fixture it is expected to pass and one it is expected to not pass, in the
language it validates, and the atlas's own calibration gate runs all of them
twice, requiring byte-identical output. A validator change is not done until
that gate is green. The reference atlas keeps its validators, fixtures, and
gate under `checks/` and documents the layout in its own README.

## Materialization and verification

Intent authoring — records, validators, and sensors — belongs to the atlas and
its own tooling. cmf and cmv read the atlas and never write to it. Once the
collection is valid, a materialization profile consumes it:

```bash
cmf assemble universal-craftsperson --explain
```

cmf scans the relevant TOML fields, filters by the profile's ecosystems,
expands only the graph relationships the profile permits, and emits an agent or
skill without changing the atlas. `cmf install --local --apply` installs that
artifact into a project and writes the **compile manifest** beside the
project's lock file: exactly which records went in, at which checksums, from
which atlas revision. cmv reads the manifest to hold the project to those
intents; see [Compiling and Verifying Intents](../guide/compiling-and-verifying.md).
