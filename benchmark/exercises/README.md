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
3. **An assembled `AGENTS.md`** — produced by running `cmf assemble` against
   the intent atlas (`--atlas <path>`, or `CMF_ATLAS`) through the scenario's
   own profile, at run time, so the artifact under test is always the current
   algorithm's output over the current records. The scenario keeps a snapshot
   of those records under `input/knowledge-base/` as the fixture that documents
   the slice; before any trial the runner checks every scored record in it
   against the atlas byte for byte and aborts, naming the keys, if they differ
   — a drift there means the guidance under test changed, and trials from
   before and after would not be measuring the same thing.

## Run

Collecting and analysing are separate commands, on purpose. The scoring rules
have changed repeatedly; re-analysis must never cost another agent invocation.

```bash
# Every collect or re-score needs the intent atlas; --atlas or this.
export CMF_ATLAS=~/Work/Projects/Personal/guidelines

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

Validate the harness itself without spending an agent invocation — both runs
go through cmv exactly as a real trial does:

```bash
# The ceiling: a checked-in solution that satisfies everything.
./benchmark/exercises/run.sh --scenario rate-card --implementation reference --arm guided

# The floor: score the untouched skeleton.
./benchmark/exercises/run.sh --scenario rate-card --skip-agent --arm guided
```

The reference must score full marks and the skeleton zero, in every scenario. A
conditional intent the skeleton reads as not applicable is a legitimate zero
(`0/7 (+1 n/a)`); an `unchecked` intent is not — it is a harness fault and the
run exits nonzero.

When a validator in the atlas is corrected, re-score what is already banked
rather than collecting again:

```bash
# Show, per archived trial, which intents would change verdict; write nothing.
python3 benchmark/exercises/rescore.py --scenario rate-card --compare

# Rewrite the adherence blocks (and the atlas revision they were scored at).
python3 benchmark/exercises/rescore.py --scenario rate-card
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
one. cmf and cmv are built once per invocation (`cargo build -p cmf -p cmv`,
binaries found through `cargo metadata`) and then run as plain binaries, so
trials never wait on cargo's target-directory lock. The cmf assembly likewise
runs once per invocation: the manifest depends only on the profile and the
atlas, and the control arm is verified against the same one. Before the pool
opens, cmv runs once over the untouched skeleton — the Rust validators build
their fact extractor into a content-addressed cache on first use, with no lock
around the build, and a run whose validators cannot answer should fail before
any agent time is spent.

## What each trial measures

**Task completion.** A hidden acceptance suite the agent never sees, copied in
after the agent has finished and run with discovery settings pinned to pytest's
defaults. It drives the public contract against a real local HTTP service and
inspects no module layout whatsoever, so any structure can pass it. This is the
control against the obvious failure mode of a style benchmark: guidance that
improves adherence while breaking the software.

**Adherence.** cmv decides, per intent, whether the finished code exhibits it,
by running the validator the intent atlas keeps beside that intent's record —
parsing the code, never asking a model and never reading the agent's
transcript. What an agent said it would do is not evidence that it did. Each
verdict ships with the validator's signals, evidence, and `path:line`
locations, so a `false` distinguishes "the guidance never arrived" from "it
arrived and was partly applied".

For cmv to verify a workspace it needs two files a real project would keep:
`.context-mixer/cmf-manifest.json`, the manifest `cmf assemble --manifest`
wrote naming the intents it composed, and `cmv.toml`, which the runner renders
from the scenario's `expected.json` — one `[intent."<key>"]` table per scored
intent carrying its `check_config` plus `baseline_root`, the untouched skeleton
whose public items the documentation validator subtracts. Both arms receive
both files, but only after the agent has finished: the manifest names the
scored intents, and a control-arm agent that could read it would know what it
was being scored on. No `ecosystems` override is written; the atlas's sensors
detect the language from the skeleton's `Cargo.toml` or `pyproject.toml`.

cmv's report is mapped onto the `adherence` and `principles` blocks the
aggregator has always read: `pass` is applicable and followed, `fail` applicable
and not, `not_applicable` neither. `unchecked` and `unguided` are not verdicts —
every scored intent has a validator for its scenario's language, so either one
means the harness failed, not the agent, and the trial is marked invalid with
the reason under `harness_faults`, exactly as a failed agent invocation is.
Each `metrics.json` also records `atlas_revision`, `cmv_exit_code`, and the
full `cmv_report`, so a later validator correction can be told apart from the
evidence it re-scored.

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
running cmv over that directory afterwards counts the harness's own file as the
agent's integration test layer. `rescore.py` removes the staged file before it
scores; do the same, or trust the `metrics.json` the run wrote.

## Where the checks live

Not here. The validators are part of the intent atlas — `checks/<language>/<slug>.py`
beside the record each one verifies, declared on the record as a `static-check`
evidence entry, with a calibration gate of their own — and cmv runs them. The
harness is one more caller of cmv; `scoring.py` is the whole of its
involvement: build the tools, guard the snapshot, assemble, stage, check, map.

What stayed with the exercise is `expected.json`'s `check_config` — the facts
only one exercise knows. Which literals mark its business rules, which symbols
count as blocking for its domain, which operations must be bounded. A constant
that would have to change per scenario belongs there, rendered into the trial's
`cmv.toml`, never in a validator. When the first scenario's fee-tier regex was
sitting in the shared scorer, the scorer was not shared; it was one scenario's
scorer with a second scenario's checks bolted on.

The checks were written here first, across three scenarios and two languages,
before they moved. Two lessons from that survive in how the atlas organizes
them. The split between question forms that generalize (is a symbol used, is it
used *there*, what shape does a construct have, what did the project declare)
and the one function per intent that asks them came out of writing the second
scenario, not out of a design. And a language change costs a fact extractor and
a partition rule, not a redesign: the Rust exercise kept every structural
decision — the check signature, the three-state verdict, the `check_config`
split, the calibration discipline — and none of the traversals, because test
scope, panicking, and substitution are each a different kind of thing in Rust
than in Python.

## Scenario contract

Each directory under `scenarios/` contains:

- `TASK.md` — the task, given to the agent verbatim.
- `input/skeleton/` — the starting project. Deliberately neutral on every
  scored decision: no existing tests, no domain models, no gateway, and default
  pytest configuration. A skeleton that demonstrates the conventions measures
  whether an agent can copy, not whether guidance works.
- `input/profile.toml` — the slice requested from the atlas: explicit keys,
  and the ecosystem the exercise targets so the ecosystem signals can fire.
- `input/knowledge-base/` — a snapshot of the records the profile selects, kept
  as the fixture that documents the slice. Scoring reads the atlas, not this;
  the runner refuses to start if a scored record here no longer matches the
  atlas's, because that means the guidance under test has changed.
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

Every scored intent must carry a validator in the atlas for the scenario's
language; the runner treats an intent cmv cannot check as a harness fault, not
as a violation. Write the reference solution before running any agent. If it
cannot pass both the acceptance suite and every validator, the scenario is not
ready.
