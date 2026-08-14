import asyncio

import httpx
import pytest
from unittest.mock import AsyncMock

from probefan.fanout import check_all_with
from probefan.models import Probe


def a_probe(name):
    return Probe(name=name, url=f"http://127.0.0.1/{name}")


async def test_expected_status_is_healthy():
    fetch = AsyncMock(return_value=200)

    results = await check_all_with([a_probe("api")], fetch)

    assert [item.outcome for item in results] == ["healthy"]
    assert results[0].status == 200


@pytest.mark.parametrize(
    ("failure", "outcome"),
    [
        (TimeoutError(), "timeout"),
        (httpx.ConnectError("refused"), "unreachable"),
    ],
)
async def test_expected_failures_become_outcomes(failure, outcome):
    fetch = AsyncMock(side_effect=failure)

    results = await check_all_with([a_probe("api")], fetch)

    assert results[0].outcome == outcome
    assert results[0].status is None


async def test_results_keep_probe_order_regardless_of_completion_order():
    async def fetch(url, timeout):
        await asyncio.sleep(0.05 if url.endswith("slow") else 0)
        return 200

    ordered = [a_probe("slow"), a_probe("quick")]

    results = await check_all_with(ordered, fetch)

    assert [item.name for item in results] == ["slow", "quick"]


async def test_one_failing_probe_does_not_suppress_the_others():
    fetch = AsyncMock(side_effect=[httpx.ConnectError("refused"), 200])

    results = await check_all_with([a_probe("gone"), a_probe("api")], fetch)

    assert [item.outcome for item in results] == ["unreachable", "healthy"]


async def test_probes_overlap_rather_than_running_one_after_another():
    async def fetch(url, timeout):
        await asyncio.sleep(0.2)
        return 200

    probes = [a_probe(f"api-{index}") for index in range(5)]

    started = asyncio.get_running_loop().time()
    await check_all_with(probes, fetch)
    elapsed = asyncio.get_running_loop().time() - started

    assert elapsed < 0.6


async def test_cancelling_the_fanout_leaves_no_pending_tasks():
    async def fetch(url, timeout):
        await asyncio.sleep(10)
        return 200

    before = len(asyncio.all_tasks())
    task = asyncio.create_task(check_all_with([a_probe("api")], fetch))
    await asyncio.sleep(0.05)
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task

    assert len(asyncio.all_tasks()) == before
