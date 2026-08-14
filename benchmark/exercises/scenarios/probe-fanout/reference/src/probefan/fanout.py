"""Concurrent fan-out over probes, each request bounded and none orphaned."""

import asyncio
from collections.abc import Iterable, Sequence
from functools import partial

import httpx

from probefan.classify import DEFAULT_EXPECTED_STATUSES, classify
from probefan.models import TIMEOUT, UNREACHABLE, Probe, ProbeResult
from probefan.transport import fetch_status

DEFAULT_TIMEOUT = 2.0


async def check_all(
    probes: Iterable[Probe],
    *,
    timeout: float = DEFAULT_TIMEOUT,
    expected_statuses: Iterable[int] = DEFAULT_EXPECTED_STATUSES,
) -> list[ProbeResult]:
    """Probe every endpoint concurrently, returning results in probe order.

    The client is opened for the whole fan-out and closed when it leaves scope,
    whether the probes succeed, time out, or the caller cancels.
    """
    async with httpx.AsyncClient() as client:
        return await check_all_with(
            probes,
            partial(fetch_status, client),
            timeout=timeout,
            expected_statuses=expected_statuses,
        )


async def check_all_with(
    probes: Iterable[Probe],
    fetch,
    *,
    timeout: float = DEFAULT_TIMEOUT,
    expected_statuses: Iterable[int] = DEFAULT_EXPECTED_STATUSES,
) -> list[ProbeResult]:
    """Probe every endpoint through an explicit fetcher.

    The task group is what makes "one probe failing must not stop the others"
    structural rather than incidental: every child is awaited before the scope
    exits, so no probe outlives the call that started it.
    """
    ordered: Sequence[Probe] = list(probes)
    acceptable = frozenset(expected_statuses)

    async with asyncio.TaskGroup() as group:
        tasks = [
            group.create_task(_probe_one(fetch, probe, timeout, acceptable)) for probe in ordered
        ]

    return [task.result() for task in tasks]


async def _probe_one(
    fetch,
    probe: Probe,
    timeout: float,
    acceptable: frozenset[int],
) -> ProbeResult:
    """Run one probe, turning its expected failures into outcomes.

    Only the two failures the contract names are caught. Cancellation is not
    among them, so a caller withdrawing the fan-out still unwinds normally.
    """
    try:
        status = await fetch(probe.url, timeout)
    except TimeoutError:
        return ProbeResult(name=probe.name, url=probe.url, outcome=TIMEOUT, status=None)
    except httpx.HTTPError:
        return ProbeResult(name=probe.name, url=probe.url, outcome=UNREACHABLE, status=None)

    return ProbeResult(
        name=probe.name,
        url=probe.url,
        outcome=classify(status, acceptable),
        status=status,
    )
