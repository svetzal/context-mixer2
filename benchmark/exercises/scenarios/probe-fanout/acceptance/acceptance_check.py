"""Hidden acceptance checks for the probe-fanout exercise.

Deliberately synchronous. Each check drives the async contract through
`asyncio.run`, so acceptance never depends on the agent having installed
pytest-asyncio — which is itself one of the scored behaviours and must not
double as a precondition for measuring the others.

Nothing here inspects module layout. Any concurrency design that satisfies the
contract passes.
"""

import asyncio
import time

import pytest

from fake_services import Services, closed_port_url

import probefan


@pytest.fixture(scope="module")
def services():
    running = Services()
    try:
        yield running
    finally:
        running.stop()


def run(coroutine):
    return asyncio.run(coroutine)


def probes(services, *paths):
    return [probefan.Probe(name=path.strip("/"), url=f"{services.url}{path}") for path in paths]


def test_expected_status_is_healthy(services):
    results = run(probefan.check_all(probes(services, "/ok")))

    assert [item.outcome for item in results] == ["healthy"]
    assert results[0].status == 200
    assert results[0].name == "ok"


def test_unexpected_status_is_unhealthy(services):
    results = run(probefan.check_all(probes(services, "/error")))

    assert results[0].outcome == "unhealthy"
    assert results[0].status == 500


def test_caller_can_widen_the_acceptable_statuses(services):
    results = run(probefan.check_all(probes(services, "/teapot"), expected_statuses={418}))

    assert results[0].outcome == "healthy"
    assert results[0].status == 418


def test_a_custom_status_set_does_not_leak_into_the_next_default_call(services):
    run(probefan.check_all(probes(services, "/teapot"), expected_statuses={418}))

    results = run(probefan.check_all(probes(services, "/ok", "/teapot")))

    assert [item.outcome for item in results] == ["healthy", "unhealthy"]


def test_a_hanging_probe_times_out(services):
    results = run(probefan.check_all(probes(services, "/slow?seconds=5"), timeout=0.5))

    assert results[0].outcome == "timeout"
    assert results[0].status is None


def test_a_closed_port_is_unreachable():
    probe = probefan.Probe(name="gone", url=closed_port_url())

    results = run(probefan.check_all([probe]))

    assert results[0].outcome == "unreachable"
    assert results[0].status is None


def test_results_keep_the_order_of_the_probes(services):
    ordered = probes(services, "/slow?seconds=1", "/ok", "/error")

    results = run(probefan.check_all(ordered, timeout=3.0))

    assert [item.name for item in results] == [item.name for item in ordered]


def test_probes_run_concurrently_rather_than_one_after_another(services):
    five_slow = probes(services, *["/slow?seconds=1"] * 5)

    started = time.monotonic()
    results = run(probefan.check_all(five_slow, timeout=4.0))
    elapsed = time.monotonic() - started

    assert [item.outcome for item in results] == ["healthy"] * 5
    assert elapsed < 3.0, f"five one-second probes took {elapsed:.2f}s; they did not overlap"


def test_one_bad_probe_does_not_suppress_the_others(services):
    mixed = [
        probefan.Probe(name="gone", url=closed_port_url()),
        *probes(services, "/ok", "/error"),
    ]

    results = run(probefan.check_all(mixed, timeout=3.0))

    assert [item.outcome for item in results] == ["unreachable", "healthy", "unhealthy"]


def test_the_timeout_bounds_the_whole_call(services):
    hanging = probes(services, *["/slow?seconds=8"] * 3)

    started = time.monotonic()
    results = run(probefan.check_all(hanging, timeout=0.5))
    elapsed = time.monotonic() - started

    assert [item.outcome for item in results] == ["timeout"] * 3
    assert elapsed < 3.0, f"a 0.5s timeout returned after {elapsed:.2f}s"
