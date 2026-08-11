# elixir-phoenix-craftsperson benchmark

This scenario asks cmf to reproduce the original Phoenix craftsperson agent
from the full 989-intent guidelines catalogue. The 95 TOML records below
`craftsperson/elixir/phoenix/` are the relevant set because each identifies the
original agent in its source scope.

The initial algorithm selects by the `phoenix` tag across ten categories. Its
baseline recovers all 95 relevant intents and retains every strategy, but also
selects `craftsperson/elixir/scan-phoenix-security-conditionally`, which came
from the separate pure-Elixir agent. That gives perfect recall and 0.9896
precision. This is a useful pressure test for provenance-aware selection.

The baseline assembly is also 1.7692 times the original byte size while
recovering 57.3% of its normalized vocabulary. That exposes the next shaping
problem: rendering rationale and evidence mechanically preserves structured
knowledge but is substantially more verbose than the source agent.

The target is full relevant-intent recall and strategy coverage with no
cross-agent false positives, followed by better source vocabulary recovery at
a materially smaller context footprint.
