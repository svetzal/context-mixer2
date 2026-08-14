# probe-fanout exercise

A service dashboard probes many endpoints on a refresh interval. One slow or
dead service must not delay the rest. The task is to fan out concurrently,
bound each probe, and classify what came back.

This is the second exercise, and it exists to answer whether the harness
generalizes. Its eight scored intents are disjoint from fx-settlement's, and
they were chosen because they are a different *kind* of question. fx-settlement
asks mostly "is this name present, is this symbol absent" — questions about
vocabulary and file layout. These ask "is this call inside that construct" and
"what shape does this argument have" — questions about control flow.

| The task forces | The intent it exposes |
| --- | --- |
| concurrent fan-out over N endpoints | `structured-concurrent-lifetimes` |
| a real per-probe bound | `graceful-async-timeouts-and-cancellation` |
| an async HTTP client | `nonblocking-async-io` |
| one client across the whole fan-out | `deterministic-resource-cleanup` |
| optional `timeout` and `expected_statuses` | `no-shared-mutable-defaults` |
| four status-to-outcome cases | `parametrized-behavior-cases` |
| testing an async collaborator | `interface-checked-mocks`, `isolated-async-tests` |

## What it added to the checker

Writing these eight is what produced `predicates.py`. The fx-settlement checks
each grew their own traversal, which was tolerable at eight and would not have
been at sixteen. Three primitives were genuinely new:

- **Containment** — `guarded_by_call` answers "is this request inside a timeout
  scope", and the same predicate answers "is this client inside a `with`". Both
  intents reduce to the same question about a different pair of nodes.
- **Context** — `in_async_context` distinguishes `time.sleep` in a synchronous
  helper, which is fine, from the same call inside `async def`, which is the
  defect.
- **Argument shape** — `defaults` and `is_mutable_default` read a construct's
  form rather than its name.

## Two places the checks are deliberately narrow

**Blocking calls.** `matches_symbol` tail-matches, so `from asyncio import
sleep; sleep(...)` would look like `time.sleep`. Awaited calls are therefore
skipped outright: an awaited call is not blocking whatever it is named. The
cost is that a genuinely blocking call someone wrapped in `await` would be
missed, which is not a thing that happens.

**Cancellation.** Only handlers that can actually intercept cancellation are
counted — bare `except`, `except BaseException`, and explicit `CancelledError`.
`except Exception` does not catch `CancelledError` on any supported Python, and
flagging it would punish the correct way to convert a transport failure into an
outcome.

## What the first real run changed

`interface-checked-mocks` failed in both arms on the first agent run. The agent
had tested everything against a live local HTTP server and used no doubles at
all, and the check — which required a double to exist so it could not pass
vacuously — read that as a violation.

The check was wrong, and wrong in a way the fx-settlement checks had hidden.
The intent is conditional: it binds a suite that substitutes something. So
verdicts gained a third state, and this check now reports `applicable: false`
when no double of any kind exists. That change is not specific to this
scenario; several intents in the corpus are conditional in the same way.

## Known gap

`isolated-async-tests` gates on async tests existing, a runner being
configured, and the runner being declared as a dependency. The intent's
evidence clause also asks for warnings treated as errors, which the checker
records as `warnings_as_errors` but does not gate on. The strategy clause is
about how to run async tests; warnings-as-errors is how you would verify the
result. Gating on it would fail solutions that follow the strategy exactly.
