# cmf behavioural exercises

The assembly benchmarks one directory up measure the artifact: how many
relevant intents were selected, how much of each record survived rendering, how
large the result is. Those are properties of a document. None of them says
whether an agent handed that document builds anything differently.

These exercises measure that instead. An agent is given a real project and a
real task, once with a cmf-assembled `AGENTS.md` and once without. What changes
between the two runs is the measurement.

## The three inputs

1. **Agent and model parameters** — a key in `agents.toml`, which holds the
   argv, guidance file locations, and isolation flags for each CLI.
2. **A scenario skeleton** — a runnable project plus a `TASK.md` written
   against a fixed public contract.
3. **An assembled `AGENTS.md`** — produced by running `cmf assemble` over the
   scenario's own intent snapshot and profile, at run time, so the artifact
   under test is always the current algorithm's output.

## Run

```bash
./benchmark/exercises/run.sh --agent claude-opus-5 --arm both --trials 3
./benchmark/exercises/run.sh --agent codex-gpt-5-6 --arm guided --trials 5
```

Validate the harness itself without spending an agent invocation:

```bash
# The ceiling: a checked-in solution that satisfies everything.
./benchmark/exercises/run.sh --implementation reference --arm guided

# The floor: score the untouched skeleton.
./benchmark/exercises/run.sh --skip-agent --arm guided
```

Results land under `benchmark/exercises/results/`, which Git ignores. Each
trial keeps its own workspace, the guidance it was given, the agent's
transcript, both pytest runs, and `metrics.json`.

## What each trial measures

**Task completion.** A hidden acceptance suite the agent never sees, copied in
after the agent has finished and run with discovery settings pinned to pytest's
defaults. It drives the public contract against a real local HTTP service and
inspects no module layout whatsoever, so any structure can pass it. This is the
control against the obvious failure mode of a style benchmark: guidance that
improves adherence while breaking the software.

**Adherence.** `adherence.py` decides, per intent, whether the finished code
exhibits it — by parsing the code, never by asking a model and never by reading
the agent's transcript. What an agent said it would do is not evidence that it
did. Each verdict ships with the signals behind it, so a `false` distinguishes
"the guidance never arrived" from "it arrived and was partly applied".

`summary.json` reports both per arm, and the per-intent lift between them.

## Arms and confounds

`control` withholds the guidance entirely; `guided` installs it. Everything else
is held constant, including the operator's own ambient configuration — a global
`CLAUDE.md`, a user `AGENTS.md`, installed skills. Ambient config is identical
across arms, so it cannot manufacture a difference; it can only raise the
control arm's floor and *understate* lift. When a guided-arm number needs to
stand on its own rather than as a delta, `--isolate-agent-home` points the
agent's configuration home at an empty scratch directory.

Two honest limitations. Model output is stochastic, so a single trial per arm
is an anecdote — run enough trials that the per-intent rates mean something.
And adherence checks recognize the shapes they were written to recognize; a
defensible design they do not anticipate scores as a violation. Read the
`signals` before believing a low score.

## Scenario contract

Each directory under `scenarios/` contains:

- `TASK.md` — the task, given to the agent verbatim.
- `input/skeleton/` — the starting project. Deliberately neutral on every
  scored decision: no existing tests, no domain models, no gateway, and default
  pytest configuration. A skeleton that demonstrates the conventions measures
  whether an agent can copy, not whether guidance works.
- `input/knowledge-base/` and `input/profile.toml` — the intents cmf sees and
  the slice requested from them.
- `acceptance/` — hidden checks, never present while the agent works.
- `reference/` — a solution satisfying every acceptance check and every scored
  intent. Its purpose is to prove the targets are simultaneously reachable; a
  benchmark nobody has ever passed is measuring its own bugs.
- `expected.json` — the scored intent keys and the acceptance check count.
- `provenance.json` — where the intent snapshot came from.

## Adding a scenario

The scored intents have to be *observable in code*. "Escalate consequential
tradeoffs" is a good intent and cannot be scored here; "configure pytest to
discover `*_spec.py`" can. Prefer intents whose behaviour is both specific and
not what a model does by default — an intent every model already follows
measures nothing, however true it is.

Write the reference solution before running any agent. If it cannot pass both
the acceptance suite and every adherence check, the scenario is not ready.
