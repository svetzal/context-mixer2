"""The imperative shell: obtain a rate, then hand the decision to the core."""

from decimal import Decimal

from fxsettle.models import Invoice, Settlement
from fxsettle.ports import RateSource
from fxsettle.pricing import settle_at_rate
from fxsettle.rates import HttpRateSource

PAR = Decimal(1)


def settle(invoice: Invoice, quote_currency: str) -> Settlement:
    """Settle an invoice into `quote_currency` at the current daily rate.

    Raises:
        UnknownCurrencyPair: the pair is not quoted by the rates service.
        RateUnavailable: the rate could not be obtained.
    """
    return settle_with(invoice, quote_currency, HttpRateSource())


def settle_with(invoice: Invoice, quote_currency: str, rates: RateSource) -> Settlement:
    """Settle an invoice using an explicit rate source.

    Raises:
        UnknownCurrencyPair: the pair is not quoted by the rate source.
        RateUnavailable: the rate could not be obtained.
    """
    if invoice.currency == quote_currency:
        return settle_at_rate(invoice, quote_currency, PAR)
    return settle_at_rate(invoice, quote_currency, rates.rate(invoice.currency, quote_currency))
