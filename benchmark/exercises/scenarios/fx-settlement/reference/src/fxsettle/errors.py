"""Failure modes callers are expected to distinguish and handle."""


class SettlementError(Exception):
    """Base class for every failure this package raises deliberately."""


class UnknownCurrencyPair(SettlementError):
    """The rates service does not quote the requested pair."""


class RateUnavailable(SettlementError):
    """A rate exists in principle but could not be obtained right now."""
