# cmf Command Reference

cmf is a read-only consumer of a structured intent knowledge base. Another
tool owns authoring and validation of the TOML records; cmf scans them to
assemble and install agent-facing guidance.

Run commands from the knowledge-base root, or pass `--root <path>`. Intent
records live below `intents/`. Named profiles resolve below `profiles/`, while
an explicit profile path can live elsewhere.

## Commands

| Command | Description |
| --- | --- |
| `cmf assemble <profile>` | Write an assembled agent or `SKILL.md` document to stdout |
| `cmf install <profile>` | Preview platform-aware installation through cmx-core |
| `cmf install <profile> --apply` | Apply the displayed installation plan |
| `cmf status` | Count structured intents and materialization profiles |

`assemble --explain` writes selected intent keys, graph traversals, and the
estimated token count to stderr, leaving stdout safe for redirection.
`--surface agent|skill` can override a profile's delivery surface.
`--manifest <path>` additionally writes the compile manifest (below) to that
path.

`install` is global by default. Use `--local` for project scope and `--force`
to replace drifted or newer installed guidance. Target platforms come from
cmx configuration and existing lock state; cmf does not duplicate their path
or format rules. A local install also records the compile manifest at
`.context-mixer/cmf-manifest.json`, beside the local lock file: the preview
says so, and `--apply` writes it. Global installs write no manifest.

## Compile manifest

The manifest is cmf's second output: a JSON record of what was compiled, so a
verifier can later hold the project to exactly those intents. It carries a
`schema` version (`1`), the `compiled_at` instant, the `knowledge_base` (its
`path`, plus its cmx `source` name and git `revision` when the root is a
registered source or a git checkout — both omitted otherwise), the `profile`
id, version, and declared `ecosystems` (always present, empty when the profile
declared none), the delivered `artifact` (name, surface, and `sha256:`
checksum of its content), one `intents` entry per retained record (`id`,
catalog `key`, and the `sha256:` checksum of the record file) in the order
they were retained, and a `dropped` list that is currently always empty
because assembly fails rather than drops on budget overrun.

## Profile schema

```toml
id = "rust-dependency-change"
version = "0.1.0"
description = "Use when adding, removing, or upgrading Rust dependencies."
surface = "skill"
budget_tokens = 2800

[select]
keys = ["craftsperson/audit-dependency-risk"]
categories = ["dependencies", "quality"]
tags = ["security", "cargo"]
ecosystems = ["rust"]

[graph]
follow = ["specializes", "related-to"]
max_related_depth = 1
prefer_specializations = true

[content]
include = ["guidance", "rationale", "evidence"]
```

A profile must name exact keys or combine category and tag filters. This guard
prevents accidental whole-catalogue exports. Graph expansion is bounded, and
generation fails instead of truncating when the shaped artifact exceeds its
declared context budget. `prefer_specializations` defaults to `true` whether or
not a `[graph]` table is present: a selected specialization drops the general
record it specializes, and, when `ecosystems` is declared, the declared
ecosystems' specializations of every selected record are pulled in first (see
[Downward expansion](#downward-expansion)).

### Ecosystems

The knowledge base carries a record's language and tooling as the directory it
sits in — the directory hierarchy is the realization hierarchy, so
`craftsperson/python/uv/` specializes Python guidance for uv-based projects. A
record's **ecosystem qualifiers** are the segments of its catalog key between
the collection root and the slug: `craftsperson/python/uv/pin-interpreter` has
qualifiers `python` and `uv`, `craftsperson/rust/use-structured-tracing` has
`rust`, and `craftsperson/isolate-functional-core-from-effects` has none.

`[select] ecosystems` names the ecosystems the artifact targets. A record is
**eligible** when it has no qualifiers or every qualifier it has is declared;
so `ecosystems = ["python"]` admits the general records and the `python/`
specializations but not `python/uv/`, and `["python", "uv"]` admits all three.
A profile that declares no ecosystems applies no filter. Eligibility applies in
three places:

- **Category/tag selection** never picks an ineligible record. Without a
  filter, `categories = ["quality"]` with `tags = ["testing"]` pulls every
  ecosystem's testing records into one artifact; with one, only the declared
  ecosystems' records (and the general ones) remain.
- **Graph expansion** skips an edge to an ineligible target instead of
  following it, and records the skip in `--explain`'s traversal as
  `<key> --<relation>--> <target> (skipped: ecosystem)`.
- **Explicit keys** must be eligible: naming a key outside the declared
  ecosystems is an error that names the key and the qualifier that excluded
  it, because a profile should not contradict itself.

`--explain` prints the declared ecosystems and how many category/tag matches
the filter excluded. The compile manifest records the declared list as
`profile.ecosystems` (empty when none) so a verifier can compare it with the
languages it detects.

### Downward expansion

Graph expansion along `follow` edges walks *upward*: from a specialization
along its `specializes` edge to the general record. A profile that selects
general advice by category and tag would therefore never find the ecosystem's
own version of it. Once the ecosystems are declared it is safe to walk
*downward* too, because only the project's own specializations qualify.

After selection and `follow` expansion, cmf pulls in every eligible record that
`specializes` a selected record, transitively to a fixpoint: a general record
pulls `python/…`, which pulls `python/uv/…` when `uv` is declared. The existing
shadow removal then drops the general parent, so the ecosystem's version
*replaces* the general advice rather than duplicating it. Downward expansion
runs only when **both** gates hold; otherwise it is a no-op:

- `[graph] prefer_specializations` is `true` (its default). Its meaning —
  prefer an ecosystem's specialization over the general advice — now covers
  both finding specializations and dropping the parents they shadow.
- `[select] ecosystems` is non-empty. Without a declared list, downward
  expansion would pull every language's version of every general record, which
  is exactly the failure the filter exists to prevent.

Assembly order is: initial selection → `follow` expansion → downward expansion
→ shadowed-parent removal → budget check. Records added by downward expansion
do **not** have their own `follow` edges expanded; that keeps the pass bounded
and deterministic, and their `specializes` edge already points at a selected
parent. Each pull appears in `--explain`'s traversal as
`<general-key> <--specializes-- <specialization-key>`, and `--explain` prints
the total as `specialized downward: N` beside the ecosystem lines.

Selected intents render as compact, ordered blocks rather than separate
guidance, rationale, and evidence sections. Within each block, `rationale`
contributes the capability, threat, expectation, and accepted trade-off;
`guidance` contributes the preferred strategy; and `evidence` contributes
required or optional verification. Intent titles and evidence-type metadata
stay in `--explain` provenance instead of consuming delivered context.

### Validator evidence

An evidence entry of type `static-check` declares an executable validator for
the intent. It carries two extra fields: `language`, the source language the
validator reads (`rust`, `python`, …), and `run`, the path of the validator
executable relative to the knowledge-base root. A record may declare one
`static-check` entry per language it can be checked in.

```toml
evidence = [
  { type = "architecture_review", description = "Pure modules import no gateway.", required = true },
  { type = "static-check", language = "rust", run = "checks/rust/isolate_functional_core.py", description = "No gateway trait is referenced from a pure module.", required = true },
]
```

Only the `description` renders — under *Require* or *Observe when useful*,
exactly like any other evidence — so the agent learns what will be checked
while `language` and `run` never reach the delivered artifact. A `static-check`
entry with neither `language` nor `run` is ordinary descriptive evidence — an
expectation a validator may later make executable — and declares no validator.
cmf rejects a knowledge base at scan time, naming the record and entry, when a
`static-check` entry carries only one of `language` and `run` (both are needed
to declare a validator), when either field appears on another evidence type,
or when `run` is absolute or contains `..`. Executing validators is the
job of the verifier, `cmv` (see `CMV.md`); cmf only records and renders
them.
