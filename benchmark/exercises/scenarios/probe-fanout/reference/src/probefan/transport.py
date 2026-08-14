"""The owned HTTP transport. Every outbound request is bounded here."""

import asyncio

import httpx


async def fetch_status(client: httpx.AsyncClient, url: str, timeout: float) -> int:
    """Return the status code for a URL, bounded by `timeout` seconds.

    Raises:
        TimeoutError: the endpoint did not answer within the bound.
        httpx.HTTPError: the endpoint could not be reached.
    """
    async with asyncio.timeout(timeout):
        response = await client.get(url)
    return response.status_code
