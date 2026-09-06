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
id and version, the delivered `artifact` (name, surface, and `sha256:`
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
declared context budget.

Selected intents render as compact, ordered blocks rather than separate
guidance, rationale, and evidence sections. Within each block, `rationale`
contributes the capability, threat, expectation, and accepted trade-off;
`guidance` contributes the preferred strategy; and `evidence` contributes
required or optional verification. Intent titles and evidence-type metadata
stay in `--explain` provenance instead of consuming delivered context.
