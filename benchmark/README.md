# cmf assembly benchmarks

This directory holds self-contained regression scenarios for cmf's intent
selection and assembly algorithms. Every scenario snapshots its inputs so a
later run measures the algorithm rather than changes in the guidelines
repository.

## Run

Run every scenario:

```bash
./benchmark/run.sh
```

Run one scenario:

```bash
./benchmark/run.sh elixir-phoenix-craftsperson
```

Generated artifacts, explanations, and metrics are written below
`benchmark/results/`, which is intentionally ignored by Git. A run fails when
snapshot integrity checks fail. Quality targets are reported separately so a
known baseline gap remains runnable while algorithms are improved.

## Scenario contract

Each directory below `scenarios/` contains:

- `input/original/`: the original hand-authored agent or skill.
- `input/knowledge-base/`: the exact structured intents visible to cmf.
- `input/profile.toml`: the assembly request under test.
- `expected.json`: immutable corpus checks and minimum outcome expectations.
- `baseline.json`: metrics from the algorithm revision that established the scenario.
- `provenance.json`: the upstream repository, revision, and copied paths.

Inputs are deliberately copied rather than referenced in place. To refresh a
scenario, replace its input snapshot deliberately, update both JSON records,
and review the resulting metric movement separately from algorithm changes.
Copied originals are excluded from Markdown auto-fixing because byte identity
is part of each scenario's integrity contract.

The scorer currently reports corpus integrity, selection precision and recall,
exact retention for every semantic intent field, vocabulary overlap, output
size, compression relative to the original, and deltas from the checked-in
baseline. These are mechanical signals, not a claim that lexical similarity
alone measures guidance quality.

Scenario inputs are fixtures, not configuration. Editing
`scenarios/*/input/profile.toml` redefines what a scenario measures, and the
metric movement that follows is indistinguishable from an algorithm regression.
Refresh a snapshot deliberately, in its own commit, with `baseline.json`
updated in the same change.

## Behavioural exercises

`benchmark/exercises/` answers the question this directory cannot: whether an
agent given the assembled guidance builds anything differently. It runs a real
coding task with and without a cmf-assembled `AGENTS.md` and reports task
completion against a hidden acceptance suite alongside per-intent adherence
measured from the finished code. See `exercises/README.md`.
