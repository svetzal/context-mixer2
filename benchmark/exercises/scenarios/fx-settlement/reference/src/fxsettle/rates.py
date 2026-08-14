"""The imperative shell's gateway to the rates service.

This module holds every external interaction and no business rule. It exists so
that specifications elsewhere substitute this contract rather than reaching into
httpx, and so that swapping the transport touches one file.
"""

import os
from decimal import Decimal, InvalidOperation

import httpx

from fxsettle.errors import RateUnavailable, UnknownCurrencyPair

RATES_URL_VARIABLE = "FXSETTLE_RATES_URL"


class HttpRateSource:
    """Reads daily rates from the configured rates service over HTTP."""

    def __init__(self, base_url: str | None = None, timeout: float = 5.0) -> None:
        self._base_url = base_url
        self._timeout = timeout

    def rate(self, base: str, quote: str) -> Decimal:
        """Return the quoted rate for one currency pair.

        Raises:
            UnknownCurrencyPair: the service does not quote this pair.
            RateUnavailable: the service is unreachable, unhealthy, or
                answered with a body that carries no usable rate.
        """
        url = self._base_url or os.environ.get(RATES_URL_VARIABLE)
        if not url:
            raise RateUnavailable(f"{RATES_URL_VARIABLE} is not set")

        try:
            response = httpx.get(
                f"{url.rstrip('/')}/v1/rates",
                params={"base": base, "quote": quote},
                timeout=self._timeout,
            )
        except httpx.HTTPError as error:
            raise RateUnavailable(f"rates service unreachable: {error}") from error

        if response.status_code == httpx.codes.NOT_FOUND:
            raise UnknownCurrencyPair(f"{base}/{quote} is not quoted")
        if response.status_code != httpx.codes.OK:
            raise RateUnavailable(f"rates service returned {response.status_code}")

        return self._parse(response, base, quote)

    def _parse(self, response: httpx.Response, base: str, quote: str) -> Decimal:
        try:
            body = response.json()
        except ValueError as error:
            raise RateUnavailable(f"rates service returned a non-JSON body: {error}") from error

        quoted = body.get("rate") if isinstance(body, dict) else None
        if quoted is None:
            raise RateUnavailable(f"rates service omitted a rate for {base}/{quote}")

        try:
            return Decimal(str(quoted))
        except InvalidOperation as error:
            raise RateUnavailable(f"rates service returned an unusable rate {quoted!r}") from error
