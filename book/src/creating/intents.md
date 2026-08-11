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

## Maintenance and materialization

Intent authoring and validation belong to the dedicated guidelines-maintenance
tool. cmf intentionally does not write, scaffold, or validate these records.
Once the collection is valid, use a materialization profile to consume it:

```bash
cmf assemble universal-craftsperson --explain
```

cmf scans the relevant TOML fields, expands only the graph relationships the
profile permits, and emits an agent or skill without changing the knowledge
base.
