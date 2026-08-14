"""The functional core: conversion, fee tiers, and currency rounding.

Nothing here reads the clock, the network, the filesystem, or the environment.
Every function returns the same result for the same arguments, which is why the
specification beside this module needs no test doubles at all.
"""

from decimal import Decimal, ROUND_HALF_UP

from fxsettle.models import Invoice, Settlement

WHOLE_UNIT_CURRENCIES = frozenset({"JPY", "KRW"})

DEFAULT_MINOR_UNITS = 2

FEE_FLOOR = Decimal(5)

FEE_TIERS = (
    (Decimal(1000), Decimal("0.025")),
    (Decimal(10000), Decimal("0.015")),
)

TOP_TIER_RATE = Decimal("0.008")


def minor_units(currency: str) -> int:
    """Return how many decimal places a currency settles to."""
    if currency.upper() in WHOLE_UNIT_CURRENCIES:
        return 0
    return DEFAULT_MINOR_UNITS


def to_currency(amount: Decimal, currency: str) -> Decimal:
    """Round an amount half-up to a currency's minor units."""
    return amount.quantize(Decimal(1).scaleb(-minor_units(currency)), rounding=ROUND_HALF_UP)


def fee_rate(gross: Decimal) -> Decimal:
    """Return the settlement fee percentage for a converted amount."""
    for ceiling, rate in FEE_TIERS:
        if gross <= ceiling:
            return rate
    return TOP_TIER_RATE


def settle_at_rate(invoice: Invoice, quote_currency: str, rate: Decimal) -> Settlement:
    """Convert an invoice at a known rate and apply the tiered settlement fee."""
    gross = to_currency(invoice.amount * rate, quote_currency)
    fee = to_currency(max(gross * fee_rate(gross), FEE_FLOOR), quote_currency)

    return Settlement(
        gross=gross,
        fee=fee,
        net=gross - fee,
        rate=rate,
        base_currency=invoice.currency,
        quote_currency=quote_currency,
    )
