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

Collecting and analysing are separate commands, on purpose. The scoring rules
have changed repeatedly; re-analysis must never cost another agent invocation.

```bash
# Collect. Trials accumulate — asking for 10 when 6 exist runs 4.
./benchmark/exercises/run.sh --scenario rate-card --agent claude-opus-5 \
    --arm both --trials 10 --concurrency 4

# Same command again after a crash resumes; --fresh discards and restarts.
./benchmark/exercises/run.sh --scenario rate-card --agent codex-gpt-5-6 \
    --arm both --trials 10 --concurrency 4

# Analyse. Reads every metrics.json under results/, no re-running.
python3 benchmark/exercises/aggregate.py --scenario rate-card
```

A `--scenario` run rewrites only that scenario's block in `comparison.json` and
leaves the others as they were, reporting what it kept. Without that,
re-analysing one scenario deletes every other scenario's numbers from the file,
and the loss is silent — afterwards it is indistinguishable from those trials
never having been run. A run at a different `--confidence` replaces the file
outright instead of merging, since intervals at two levels must not sit in one
report claiming a single confidence.

Validate the harness itself without spending an agent invocation:

```bash
# The ceiling: a checked-in solution that satisfies everything.
./benchmark/exercises/run.sh --implementation reference --arm guided

# The floor: score the untouched skeleton.
./benchmark/exercises/run.sh --skip-agent --arm guided
```

## Durability

A trial costs a real agent invocation and cannot be recreated — models are
stochastic, and the one that produced a result may not be served next month. So
the evidence is the artifact and the workspace is scaffolding.

Every completed trial is archived to `benchmark/exercises/archive/`, which Git
**tracks**, as a plain `metrics.json` plus an `evidence.tar.gz` holding the
guidance given, the transcript, the test output, and the workspace source. About
46 KB per trial. `results/` stays ignored and disposable.

Build output is pruned once the archive is written — `.venv`, `target/`,
`__pycache__` and friends. That is 99.5% of a finished trial: a Python trial goes
from 23 MB to 200 KB and a Rust one from 67 MB to about the same, with the source
kept because every check correction in this project came from reading it
afterwards. `--keep-workspace` opts out for debugging.

Three properties follow, each verified:

- **Analysis survives a wiped `results/`.** `aggregate.py` reads the archive
  first and the working tree second, deduplicating by
  scenario/agent/arm/trial.
- **Resume counts archived trials.** Clearing `results/` does not cause a sweep
  to re-run work whose evidence is already banked.
- **A 300-trial sweep costs ~14 MB archived** instead of 10–20 GB of
  regenerable build output.

This was not hypothetical. Earlier in this project's history a `rm -rf results`
during a refactor destroyed six completed agent trials, because the only copy
was in an ignored directory.

Calibration runs land under `results/<scenario>/_calibration/` and are marked
`kind: "calibration"`, so they cannot leak into a rate. Agent trials land under
`results/<scenario>/<agent>/<arm>/trial-NN/`, keyed by agent so one model's
sweep never overwrites another's. Each trial keeps its own workspace, the
guidance it was given, the agent's transcript, both test runs, and
`metrics.json`. All of it is Git-ignored.

## Distributions, and comparing models

A single trial per arm is an anecdote, and a bare rate hides which one you are
looking at. `aggregate.py` reports every rate with its `n` and a Wilson score
interval, and every lift as a Newcombe interval on the difference of two
proportions — so "the guidance helped" is a claim with a width.

```text
  claude-sonnet-5  [served: claude-sonnet-5]  cost $3.42
    control  n=10  adherence 0.14 [0.05, 0.36]  complete 1.0
    guided   n=10  adherence 0.88 [0.66, 0.96]  complete 1.0
    lift     0.74 [0.42, 0.89] at 95% — excludes 0
```

`excludes_zero` is the honest headline. Every lift reported earlier in this
project's history was measured at n=1, and all of them include zero.

**Model identity is recorded, not assumed.** `--model sonnet` is an alias whose
target moves, and a session can route part of its work elsewhere. Each trial
stores what the CLI says it actually used — resolved model ids, per-model token
counts, and cost where the CLI reports one. Claude Code reports all three; codex
reports tokens but no model or cost, so the model is taken from argv and cost is
recorded as unknown rather than zero.

Adding a model is one entry in `agents.toml`, and the aggregator groups by agent
automatically. Comparing six models is six collect commands and one analyse.

### Which driver runs which model

Anthropic models run under Claude Code and OpenAI models under Codex, because
both authenticate against a subscription rather than metered API keys. opencode
is used only for local ollama weights. Routing a hosted model through a
third-party client would move the same work onto per-token billing, so nothing
here does that.

One consequence for reading results: the `cost_usd` a hosted trial records is
*notional* — the API-equivalent price of those tokens, not money billed. On a
subscription the binding constraint for a large sweep is rate limits, not
dollars.

### Local models

Three adapters drive local weights through `opencode run` against ollama. They
need care that hosted APIs do not:

- **`max_concurrency`** is a per-agent clamp the runner applies over
  `--concurrency`. One ollama instance holding 18–81 GB of weights cannot serve
  parallel trials, and two trials wanting different models would measure
  eviction rather than the models. The local adapters declare `1`.
- **`warmup`** sends one throwaway request before the sweep. A cold model takes
  minutes to load — long enough that the first trial otherwise times out
  measuring the loader rather than inference.
- **`OPENCODE_CONFIG`** points at `opencode-config.json` in this directory, so
  the harness declares the provider and models it needs. The operator's global
  opencode config is neither read nor modified, which is both isolation and the
  only way to reach a model opencode's catalog does not list.
- **Cost is zero, not unknown.** Local inference reports `cost: 0` per step,
  which is true at the margin. That is different from codex, which reports no
  cost at all and is recorded as `null`.

Verify a model can actually be driven before adding it. All three here emit
proper tool calls through the OpenAI-compatible endpoint, which is the
capability the harness depends on — a model that cannot call tools cannot edit
files, and would score zero for a reason that has nothing to do with guidance.

**Concurrency.** Trials are independent and workspace-isolated, so
`--concurrency N` runs N at once; two trials complete in roughly the time of
one. The cmf assembly runs once per invocation rather than per trial, because
concurrent `cargo run` in the repo would serialize on a single target-directory
lock for bytes that are identical anyway.

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

Verdicts have three states, not two. Some intents are conditional: "mock only
owned boundaries" binds code that mocks something. An agent that tested
everything against a live server has no doubles to spec, and scoring that as a
violation says something false about its work. Conditional intents whose
condition never arose report `applicable: false` and leave the denominator, so
an adherence rate always reads "of the intents this work had occasion to
exhibit". `not_applicable` is reported alongside every rate — a slice that is
mostly inapplicable is telling you the profile is wrong for the task.

`summary.json` reports both per arm, and the per-intent lift between them.

## Arms and confounds

`control` withholds the guidance entirely; `guided` installs it. Everything else
is held constant, including the operator's own ambient configuration — a global
`CLAUDE.md`, a user `AGENTS.md`, installed skills. Ambient config is identical
across arms, so it cannot manufacture a difference; it can only raise the
control arm's floor and *understate* lift. When a guided-arm number needs to
stand on its own rather than as a delta, `--isolate-agent-home` points the
agent's configuration home at an empty scratch directory.

Three honest limitations. Model output is stochastic, so a single trial per arm
is an anecdote — run enough trials that the per-intent rates mean something.
Adherence checks recognize the shapes they were written to recognize; a
defensible design they do not anticipate scores as a violation. Every scenario
so far has had checks corrected by its first real run — six corrections across
three scenarios, and every one of them went the same way: the check was stricter
than the intent and the agent was right. Treat a violation as a claim to verify
against the `signals`, not as a finding.

And do not re-score a finished workspace by hand without accounting for staging.
The Rust hidden suite is copied into `workspace/tests/` after adherence has run;
re-running `adherence.py` over that directory afterwards counts the harness's own
file as the agent's integration test layer. Delete the staged file first, or
trust the `metrics.json` the run wrote.

## How the checks are built

Four modules, split along the line between what generalizes and what does not.

`predicates.py` holds the traversals. They are question forms, not answers:
*is a symbol used at all* (`calls_to`, `references`, `imported_roots`), *is it
used there* (`guarded_by_call`, `in_async_context`, `within`), *what shape does
this construct have* (`defaults`, `keyword_map`, `decorator_names`,
`class_shape`), and *what did the project declare* (`tool_config`, which finds
a setting in `pyproject.toml`, `pytest.ini`, `setup.cfg`, or `tox.ini` without
the caller caring which).

`checks.py` holds one function per intent. Each takes `(workspace, config)` and
returns a verdict, its signals, and evidence.

`expected.json` holds `check_config` — the facts only one exercise knows. Which
literals mark its business rules, which symbols count as blocking for its
domain, which operations must be bounded. A constant that would have to change
per scenario belongs there. When the first scenario's fee-tier regex was
sitting in the shared scorer, the scorer was not shared; it was one scenario's
scorer with a second scenario's checks bolted on.

The split was not designed up front. It came out of writing the second
scenario, where three questions the first had never asked — containment,
syntactic context, and argument shape — would otherwise have grown three more
bespoke walks.

`rustfacts/` is a small `syn` binary that emits the same facts for Rust as JSON,
because Python's `ast` does not reach that far. The runner builds it on demand
and only for Rust scenarios.

### What a language change does and does not cost

The Rust exercise was built to find out. The four question forms survived, as
did every structural decision: the check signature, the three-state verdict, the
`check_config` split, and the calibration discipline. The traversals did not
survive at all.

Three assumptions turned out to belong to Python rather than to the intents.
Test scope is a directory in Python and an attribute in Rust, so the
production/test partition is per module there and per *item* here. Panicking is
a call in Python and a macro in Rust, invisible to anything that only walks call
expressions. Substituting a collaborator is patching a name in Python and
implementing a trait in Rust — a relationship between two definitions rather
than a string argument.

The lesson for a fourth language: budget for a fact extractor and for the
partition rule, not for redesigning the checks.

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
- `expected.json` — the scored intent keys, the acceptance check count, and the
  `check_config` block carrying anything scenario-specific the checks need.
- `provenance.json` — where the intent snapshot came from.

Three scenarios exist, with disjoint intent sets — two exercises scoring the
same intents would measure the harness twice and the guidance once.

| Scenario | Language | Scores |
| --- | --- | --- |
| `fx-settlement` | Python | structure, naming, typing |
| `probe-fanout` | Python | concurrency, cancellation, resource lifetime |
| `rate-card` | Rust | errors, effect boundaries, test layers, lint policy |

A scenario declares its `language` in `expected.json`. Python scenarios build
with `uv` and run pytest; Rust scenarios build with `cargo` and run its hidden
suite as an integration test staged into the crate *after* adherence has been
scored, since a Rust integration test has to live inside the crate to run.

## Adding a scenario

The scored intents have to be *observable in code*. "Escalate consequential
tradeoffs" is a good intent and cannot be scored here; "configure pytest to
discover `*_spec.py`" can. Prefer intents whose behaviour is both specific and
not what a model does by default — an intent every model already follows
measures nothing, however true it is.

Write the reference solution before running any agent. If it cannot pass both
the acceptance suite and every adherence check, the scenario is not ready.
