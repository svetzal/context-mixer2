"""The contracts the core depends on, owned here rather than by a vendor."""

from decimal import Decimal
from typing import Protocol


class RateSource(Protocol):
    """Supplies the daily rate for one currency pair."""

    def rate(self, base: str, quote: str) -> Decimal:
        """Return the rate converting one unit of `base` into `quote`.

        Raises:
            UnknownCurrencyPair: the pair is not quoted.
            RateUnavailable: the rate could not be obtained.
        """
