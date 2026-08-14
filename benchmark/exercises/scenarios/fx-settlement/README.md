# fx-settlement exercise

A small accounts-receivable package needs cross-currency settlement: convert an
invoice at a fetched daily rate, apply a tiered fee, round to the quote
currency's minor units, and fail in two distinguishable ways.

The task is ordinary. It was chosen because doing it at all forces every one of
the eight scored decisions:

| The task forces | The intent it exposes |
| --- | --- |
| an HTTP call to an outside service | `functional-core-imperative-shell`, `gateway-only-mocking` |
| tiered fee arithmetic and rounding | `functional-core-imperative-shell` |
| `Invoice` and `Settlement` values | `immutable-domain-models` |
| annotating a new public API | `native-modern-type-syntax` |
| two named failure modes | `specific-domain-errors` |
| writing the tests | `colocated-module-specifications`, `readable-bdd-specifications`, `native-pytest-assertions` |

Every one of these has a default an unguided model reaches for — `tests/` over
colocated `*_spec.py`, `test_*` functions over `Describe`/`should_`,
`Optional[X]` over `X | None`, a mutable dataclass over a frozen model,
`patch("httpx.get")` over an owned gateway. That gap is what makes the exercise
able to detect anything. An intent whose behaviour the model already produces
would score 1.0 in both arms and tell us nothing.

## Neutrality of the skeleton

`input/skeleton/` carries a `pyproject.toml`, a README, and one module of
static currency reference data with no type annotations, no tests, and no
models. This is deliberate. Any precedent in the skeleton — one existing spec
file, one annotated signature — would be copied by a competent agent in both
arms, and the exercise would measure imitation rather than guidance.

The consequence is that the fee tiers, the rounding rule, and the failure
contract all have to be specified in `TASK.md` precisely enough for the hidden
acceptance suite to assert exact values. `TASK.md` is a specification, not a
tutorial: it says nothing about project structure, testing style, typing, or
error taxonomy.

## Rounding is a real check, not a trick

One acceptance check settles 5,000 USD at 1.3542, where the fee is exactly
101.565. It rounds to 101.57 under decimal arithmetic and to 101.56 under
binary floating point. `TASK.md` states that monetary values are `Decimal` and
that rounding is half-up, so the check is fair — but it is the one place where
a plausible implementation silently produces a wrong number.

## Known gap

`craftsperson/python/gateway-only-mocking` is scored as followed when an owned
gateway abstraction exists and no test double targets a third-party library.
An implementation that writes no tests at all trivially avoids patching httpx,
so this check leans on the gateway half of the intent. The `substitution_style`
signal in `metrics.json` records which of `owned-substitute`, `third-party-patch`,
`other-patch`, or `none` actually occurred; read it before reading the verdict.
