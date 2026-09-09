# Deterministic Verification of Intents

> **Status:** Living design document. Published in the open on purpose — it
> records *why* Context Mixer grew a verifier, and why it verifies the way it
> does, so users and contributors can see the design goals and trade-offs
> rather than reverse-engineering them. The working note this chapter distils,
> with its phased plan and open decisions, is
> [`CMV.md`](https://github.com/svetzal/context-mixer2/blob/main/CMV.md) in
> the repository. Last substantive update: 2026-09.

## Why this document exists

cmf compiles a set of intent records into the guidance a project's agents
read. Whether the project's code then *holds those intents true* is a separate
question, and for a long time only the behavioural benchmark answered it — by
parsing finished code with one deterministic check per intent, never by asking
a model.

Those checks proved three things worth keeping:

- **Parsing beats asking.** A verdict derived from the code is reproducible,
  cheap, and cannot be talked into agreement. What an agent said it would do
  is not evidence that it did.
- **The question forms generalize; the substrate does not.** Python and Rust
  checks share one verdict shape, one three-state result, and one split between
  what the check knows and what only the project knows — but Rust needed its
  own fact extractor. Each language is best at reading its own source.
- **Checks get corrected, often.** One correction reversed a headline that
  guidance had broken the software. Evidence has to survive its own scoring
  rules.

But the checks lived inside the benchmark, and nothing outside it could run
them. A project that installed cmf guidance had no way to ask whether its code
still followed it. **cmv** makes that question a normal part of quality
verification, run from a project root like a linter, and this chapter records
the shape that makes it trustworthy.

## The intent atlas

The knowledge base cmf compiles from and cmv verifies against has a name, the
**intent atlas**, and four parts, all read-only to the tooling:

- **Ecosystems** it supports, each with the **sensors** that recognize it at a
  project root and the ecosystems it implies (`uv` implies `python`).
- **Intent records**, placed in the directory hierarchy that names their
  ecosystem: `craftsperson/python/uv/` specializes Python guidance for uv
  projects, and that nesting *is* the record's ecosystem.
- **Validators** beside the intents, one per language that can check them.
- **Profiles** that name a slice of it for a delivery surface.

Someone who forks the atlas forks all four. The tooling carries no opinion
about software; every opinion is in the atlas, and the tooling verifies a
forked or replaced atlas exactly as well.

## Three responsibilities

| | Owns | Reads | Writes |
| --- | --- | --- | --- |
| **Intent atlas** | Ecosystems and sensors; records and validators; profiles | — | Sensors, records, validators, calibration fixtures |
| **cmf** (composes) | Selection and rendering | Atlas, profile | Guidance artifact, **compile manifest** |
| **cmv** (verifies) | Dispatch, aggregation, exit code | Manifest, project config, workspace | Nothing |

One library, the `intent-atlas` crate, is the reader of the atlas's shape that
cmf and cmv share — catalog, profiles, selection, sensors, manifest. Neither
binary depends on the other. It is deliberately not part of `cmx-core`: cmx has
no use for it, and cmx-core's TypeScript twin and lockstep release would be
paid for a consumer that does not exist.

## Design decisions

1. **cmv is purely deterministic.** Given the same workspace, manifest, and
   project configuration, its output is byte-identical. It has no LLM feature,
   never falls back to a model when a validator is missing, reads nothing
   outside its inputs, and executes no project code. This holds by
   construction, not by convention.
2. **The compile manifest is cmf's second output artifact.** Not a debugging
   aid: a schema with a version, written beside the project's lock file,
   committed with the project, and pinned by golden fixtures. It is the
   `--explain` output made durable and given a contract.
3. **Validators live in the atlas, beside their records**, declared as evidence
   entries. cmv carries no opinion about any intent, so it stays stable while
   validators keep being corrected.
4. **One validator per language per intent.** A record may declare several;
   cmv runs the ones matching the workspace's detected ecosystems. A language
   with no validator reports *unchecked*, never *pass*.
5. **The atlas is the registry, pinned by revision.** Vendoring validator code
   into projects would strand every correction; an unpinned live registry would
   make CI irreproducible. A pin gives both: cmv verifies at the revision the
   manifest recorded, materializing that tree when the checkout has moved on,
   and reports the move without changing the exit code.
6. **Static analysis first.** Every existing validator is static, including the
   ones that sound like runtime properties. Runtime evidence, when it comes,
   will arrive through the project's own test runner, inheriting its
   determinism standard rather than inventing one.
7. **`required` governs gating; a record's maturity is informational.**

## The pieces

**A validator on a record** is an evidence entry with an executable:

```toml
evidence = [
  { type = "architecture_review", description = "Core modules depend on project-owned gateway contracts rather than concrete vendor clients.", required = true },
  { type = "static-check", language = "rust", run = "checks/rust/put-gateways-at-effect-boundaries.py", description = "A project-owned gateway trait is declared, implemented by a concrete adapter, and depended upon by at least one module that performs no I/O.", required = true },
]
```

The description is what the agent sees — cmf renders it into the guidance — so
the agent knows what will be checked. `run` and `language` never reach the
artifact. A `static-check` entry with neither field is ordinary descriptive
evidence, the kind a validator later makes executable.

**The invocation protocol** is a subprocess with two arguments, `--workspace`
and `--config`, and one JSON document on stdout: `applicable`, `followed`,
`signals`, `evidence`, `locations`. Anything else — a crash, a timeout,
unparseable output — is *unchecked* with the reason. cmv never guesses.

**Verdicts have three states, not two.** Some intents are conditional: "mock
only owned boundaries" binds code that mocks something. A suite that tested
everything against a live server has no doubles to spec, and scoring that as a
violation says something false about the work. Not-applicable intents leave
the denominator, so an adherence rate always reads "of the intents this work
had occasion to exhibit."

**Project configuration** holds what neither the atlas nor the code can
supply — which literals mark the business rules, which symbols count as
blocking — as hand-authored TOML at the project root, the way linters keep
theirs. Machine-written state is JSON in the state directory.

**Sensors** make ecosystem detection the atlas's, not the tooling's. A file at
the atlas root declares each ecosystem's signatures in a few predicate kinds
(a file exists, a glob matches, a file contains a literal) and its
implications. The same detected set drives cmf's warning that a profile
targets an ecosystem the project lacks, cmv's choice of validators, and cmv's
note that a manifest was compiled for an ecosystem the workspace does not
contain. An atlas with no sensors detects nothing, and both tools say so.

**Ecosystem eligibility** in cmf follows the same hierarchy: a profile declares
the ecosystems it targets, a record is eligible when every ecosystem in its
path is declared, and category-and-tag selection never picks another
language's advice. With that filter in place cmf can also walk `specializes`
edges *downward*, so general advice compiles to the ecosystem's own version of
it and the general parent is dropped.

## Calibration

A validator that cannot fail measures nothing. Every validator in the atlas
ships with a fixture it passes and one it does not, in the language it
validates, and the atlas's own gate runs all of them twice and requires
byte-identical output. That is where the determinism rule is enforced, and it
is also why cmv can trust what it dispatches: the atlas has already proved each
validator can fail.

## What happened to the benchmark

The behavioural benchmark became a consumer of the same tools rather than the
owner of the checks. It compiles each scenario's profile with cmf, stages the
manifest and project configuration into every trial workspace, and scores with
`cmv check`. Rescoring an archive of hundreds of agent trials through cmv
reproduced the stored verdicts exactly — and, in doing so, found one real
validator bug the benchmark's own scorer had been masking. That is the
feedback loop the design exists for: a check corrected for cmv is corrected for
the benchmark in the same commit, and every project running cmv in CI is a
calibration source.

## What this deliberately is not

- Not a model judging adherence. cmv parses; it never asks.
- Not a runtime monitor. cmv never executes the project.
- Not an author of the atlas. cmf and cmv consume records, validators, and
  sensors read-only; the atlas's own tooling owns them.
