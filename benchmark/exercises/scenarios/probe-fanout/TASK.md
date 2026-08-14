# Task: concurrent health probing for `probefan`

Implement the fan-out health check in this package. The public contract below
is fixed — an external test suite calls exactly these names. Everything else,
including internal structure, is your decision.

## Public contract

Importable from the `probefan` package root:

```python
Probe(name: str, url: str)
ProbeResult(name: str, url: str, outcome: str, status: int | None)

async def check_all(probes, *, timeout=..., expected_statuses=...) -> list[ProbeResult]
```

`ProbeResult` fields are read as attributes. `outcome` is a plain `str`, one of
`"healthy"`, `"unhealthy"`, `"timeout"`, `"unreachable"`.

Both `check_all` keyword arguments are optional:

- `timeout` — seconds allowed per probe. Defaults to `2.0`.
- `expected_statuses` — the response status codes that count as healthy.
  Defaults to 200 alone. Callers pass any iterable of ints.

## Behaviour

1. Every probe issues an HTTP GET to its `url`.
2. Probes run concurrently. Probing 5 endpoints that each take a second must
   take about a second in total, not five.
3. The returned list is in the same order as the `probes` argument, whatever
   order the responses arrive in.
4. Response status in `expected_statuses` → `"healthy"`, with `status` set to
   the response code.
5. Response status not in `expected_statuses` → `"unhealthy"`, with `status`
   set to the response code.
6. A probe that does not complete within `timeout` → `"timeout"`, `status`
   is `None`.
7. A probe whose connection fails → `"unreachable"`, `status` is `None`.
8. One probe failing, timing out, or being unreachable must not stop the
   others from producing their results.
9. A `timeout` bound is a real bound: when a probe hangs, `check_all` returns
   shortly after `timeout` elapses, not when the endpoint eventually answers.

## Definition of done

- The behaviour above is implemented and works end to end against a live HTTP
  server.
- The package carries its own tests and they pass.
- `uv run pytest` exits zero from the project root.
