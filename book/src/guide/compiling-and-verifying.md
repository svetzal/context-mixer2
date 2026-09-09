# Compiling and Verifying Intents

cmx installs agents and skills that someone already wrote. cmf and cmv close a
different loop: cmf **compiles** engineering intents from an intent atlas into
the guidance a project's agents read, and cmv **verifies**, from the project's
source alone, that the code holds those intents. This page walks the loop once,
end to end. The flags are in the [cmf](../reference/cmf-commands.md) and
[cmv](../reference/cmv-commands.md) references.

## What you need

- **An intent atlas** — a git repository of intent records with validators
  beside them and an `ecosystems.toml` declaring what it can recognize. The
  reference atlas is [svetzal/guidelines](https://github.com/svetzal/guidelines).
  Register it as a cmx source so both tools can find it by name:

  ```bash
  cmx source add guidelines https://github.com/svetzal/guidelines
  ```

- **A profile** — the TOML file that names the slice of the atlas your project
  wants: which intents, for which ecosystems, on which delivery surface. A
  profile lives in the atlas under `profiles/` or anywhere you keep it; see
  [Profile schema](../reference/cmf-commands.md#profile-schema).
- **cmf and cmv on your PATH** — both ship with cmx (see
  [Installation](../getting-started/installation.md)).

## 1. Compile guidance into the project

From the project root, with the atlas checked out at `~/atlas`:

```bash
cmf --root ~/atlas install profiles/rust-service --local
```

The preview shows what will be written. It also senses the project's
ecosystems with the atlas's sensors and warns when the profile targets one the
project does not show — compiling a Rust profile into a Python service is
almost always the wrong profile. When the plan looks right:

```bash
cmf --root ~/atlas install profiles/rust-service --local --apply
```

Two things land:

- The assembled agent or skill, installed into the project-local locations of
  your configured platforms — the same places `cmx agent install --local`
  uses.
- `.context-mixer/cmf-manifest.json`, the **compile manifest**: exactly which
  intents went into the guidance, each record's checksum, the profile and its
  ecosystems, and the atlas revision cmf read. This is the contract cmv
  verifies against. **Commit it.** A project without a manifest has nothing to
  be held to.

`cmf assemble <profile> --explain` shows the selection without installing
anything, including which records the ecosystem filter excluded and which
specializations were pulled in for your ecosystems.

## 2. Tell the validators what only your project knows

Most validators read the code and need nothing else. A few need a fact the
atlas cannot know — which literals mark your business rules, which symbols
count as blocking in your domain. Those go in `cmv.toml` at the project root,
one table per intent, handed to that intent's validators verbatim:

```toml
[intent."craftsperson/rust/isolate-functional-core-from-effects"]
business_rule_pattern = "\\b10_?000\\b|\\b500\\b|\\b20\\b"
business_rule_minimum_matches = 2
```

Skip the file entirely if no validator asks for anything; every intent then
receives an empty configuration. `cmv explain <intent>` shows the exact
document a validator would receive.

## 3. Verify

```bash
cmv check
```

cmv reads the manifest, finds the atlas (through the cmx source registry, or
the path the manifest recorded, or `--atlas <path>`), detects the project's
ecosystems with the atlas's sensors, runs each compiled intent's validators
for those ecosystems, and reports one line per intent:

```text
PASS       craftsperson/rust/put-gateways-at-effect-boundaries
           src/source.rs:10
FAIL       craftsperson/rust/use-structured-tracing  (required)
           production code uses println! at src/lib.rs:42
           src/lib.rs:42
N/A        craftsperson/rust/prefer-fakes-at-boundaries
           no tests, so nothing was substituted either way

1 pass, 1 fail, 1 not applicable, 0 unchecked, 0 unguided; adherence 50.0%
```

Every verdict comes from a validator parsing the code. Nothing asks a model,
reads a transcript, or runs your project. The same inputs produce the same
output, byte for byte.

The states, and what they do to the exit code:

| State | Meaning | Exit |
| --- | --- | --- |
| `PASS` | The code holds the intent | — |
| `FAIL` | It does not | `1` when the validator is required |
| `N/A` | The intent's condition never arose (a "mock only owned boundaries" rule against a suite with no mocks) | — |
| `UNCHECKED` | No verdict: no validator for your ecosystems, or the validator crashed, timed out, or could not start — the reason is printed | `1` only with `--strict` |
| `UNGUIDED` | The manifest dropped this intent, so the guidance never reached the project | — |

Exit `0` means every required, applicable intent held. Exit `2` means cmv could
not reach a verdict at all: no manifest, an unreadable atlas, bad usage.
`--json` emits the full report for CI and for tooling.

## 4. Put it in CI

cmv is a linter as far as CI is concerned. It needs the atlas at the revision
the manifest pinned, so clone the atlas with history rather than shallowly:

```yaml
- uses: actions/checkout@v5
- uses: actions/checkout@v5
  with:
    repository: svetzal/guidelines
    path: atlas
    fetch-depth: 0
- run: cmv check --atlas ./atlas --strict
```

`--strict` turns an unchecked intent into a failure, which is what you want in
CI: a validator that could not run should not read as a pass. Leave it off
locally while a language's validators are still being written.

## 5. When the atlas moves

The manifest pins the atlas revision cmf compiled from. When the atlas's `HEAD`
has moved past that pin, cmv still verifies against the **pinned** tree — it
materializes that revision into a scratch directory and runs the validators
from there — so a corrected validator upstream never silently changes a
verdict you already have. The report then adds one informational line:

```text
atlas has moved: HEAD 9f3c1e2a7b04 vs pinned a1b2c3d4e5f6; 2 compiled records or validators changed; re-run cmf install to recompile
```

Intents whose record or validator changed at `HEAD` are marked **stale**. None
of this changes the exit code. The remedy is always to recompile with
`cmf install`, because a newer atlas may change *which* intents apply, and that
decision belongs to cmf. cmv never re-pins on its own.

`cmv status` shows the pin, the atlas's `HEAD`, which tree was verified, the
detected ecosystems, and how many compiled intents have a validator, without
running anything.

## 6. Understanding one intent

```bash
cmv explain craftsperson/rust/use-structured-tracing
```

shows how the manifest entry resolved to a record, every validator the record
declares, which of them would run for the detected ecosystems and with exactly
which command line, the configuration document they would receive, and
whether the record is stale. Use it when a verdict surprises you, before
changing either the code or the atlas.

## For validator authors

While correcting a validator in a working copy of the atlas, verify against
that working tree instead of the pin:

```bash
cmv check --atlas ~/atlas --at-head
```

A validator change is not done until the atlas's own calibration gate is green:
every validator ships with a fixture it passes and one it does not, and the
gate runs them all twice and requires identical output. See
[Writing Intents](../creating/intents.md#validators).
