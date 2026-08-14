from decimal import Decimal

import pytest

from fxsettle.errors import RateUnavailable, UnknownCurrencyPair
from fxsettle.models import Invoice
from fxsettle.settlement import settle_with


class StubRateSource:
    """An owned RateSource, so these specifications never touch httpx."""

    def __init__(self, rate=Decimal("1.3542"), failure=None):
        self._rate = rate
        self._failure = failure
        self.calls = []

    def rate(self, base, quote):
        self.calls.append((base, quote))
        if self._failure is not None:
            raise self._failure
        return self._rate


def an_invoice(amount, currency="USD"):
    return Invoice(amount=Decimal(amount), currency=currency, counterparty="Acme")


class DescribeSettlingAnInvoice:
    def should_ask_the_rate_source_for_the_invoice_pair(self):
        rates = StubRateSource()

        settle_with(an_invoice("100.00"), "CAD", rates)

        assert rates.calls == [("USD", "CAD")]

    def should_apply_the_rate_the_source_returned(self):
        settlement = settle_with(an_invoice("500.00"), "CAD", StubRateSource())

        assert settlement.rate == Decimal("1.3542")
        assert settlement.gross == Decimal("677.10")


class DescribeSettlingWithinOneCurrency:
    def should_settle_at_par(self):
        settlement = settle_with(an_invoice("100.00"), "USD", StubRateSource())

        assert settlement.rate == Decimal("1")
        assert settlement.gross == Decimal("100.00")

    def should_not_consult_the_rate_source_at_all(self):
        rates = StubRateSource()

        settle_with(an_invoice("100.00"), "USD", rates)

        assert rates.calls == []


class DescribeRateFailures:
    def should_surface_an_unquoted_pair_to_the_caller(self):
        rates = StubRateSource(failure=UnknownCurrencyPair("USD/GBP"))

        with pytest.raises(UnknownCurrencyPair):
            settle_with(an_invoice("100.00"), "GBP", rates)

    def should_surface_an_unreachable_service_to_the_caller(self):
        rates = StubRateSource(failure=RateUnavailable("upstream down"))

        with pytest.raises(RateUnavailable):
            settle_with(an_invoice("100.00"), "CAD", rates)
