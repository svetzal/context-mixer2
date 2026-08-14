"""Hidden acceptance checks for the fx-settlement exercise.

These never exist in the workspace while the agent works. The runner copies
them into a directory outside the workspace at scoring time and runs them with
discovery settings pinned to pytest's defaults, so whatever collection
conventions the agent configured cannot affect whether they run.

Every check goes through the public contract in TASK.md and a real HTTP rates
service. Nothing here inspects module layout, so an implementation is free to
be structured any way at all and still pass.
"""

from decimal import Decimal

import pytest

from fake_rates import RatesService

import fxsettle


@pytest.fixture(scope="module")
def rates():
    service = RatesService()
    try:
        yield service
    finally:
        service.stop()


@pytest.fixture(autouse=True)
def rates_url(rates, monkeypatch):
    rates.clear()
    monkeypatch.setenv("FXSETTLE_RATES_URL", rates.url)
    return rates.url


def invoice(amount, currency="USD"):
    return fxsettle.Invoice(
        amount=Decimal(amount),
        currency=currency,
        counterparty="Acme Manufacturing",
    )


def test_low_tier_charges_two_and_a_half_percent():
    settlement = fxsettle.settle(invoice("500.00"), "CAD")

    assert settlement.rate == Decimal("1.3542")
    assert settlement.gross == Decimal("677.10")
    assert settlement.fee == Decimal("16.93")
    assert settlement.net == Decimal("660.17")
    assert settlement.base_currency == "USD"
    assert settlement.quote_currency == "CAD"


def test_mid_tier_charges_one_and_a_half_percent_with_half_up_rounding():
    # 6771.00 * 0.015 is exactly 101.565, which only rounds to 101.57 when the
    # arithmetic is decimal rather than binary floating point.
    settlement = fxsettle.settle(invoice("5000.00"), "CAD")

    assert settlement.gross == Decimal("6771.00")
    assert settlement.fee == Decimal("101.57")
    assert settlement.net == Decimal("6669.43")


def test_high_tier_charges_zero_point_eight_percent():
    settlement = fxsettle.settle(invoice("20000.00"), "CAD")

    assert settlement.gross == Decimal("27084.00")
    assert settlement.fee == Decimal("216.67")
    assert settlement.net == Decimal("26867.33")


def test_fee_never_falls_below_five_units():
    settlement = fxsettle.settle(invoice("10.00"), "CAD")

    assert settlement.gross == Decimal("13.54")
    assert settlement.fee == Decimal("5")
    assert settlement.net == Decimal("8.54")


def test_zero_minor_unit_currency_rounds_to_whole_units():
    settlement = fxsettle.settle(invoice("1000.00"), "JPY")

    assert settlement.gross == Decimal("157200")
    assert settlement.fee == Decimal("1258")
    assert settlement.net == Decimal("155942")


def test_same_currency_settles_at_par_without_calling_the_service(rates):
    settlement = fxsettle.settle(invoice("100.00"), "USD")

    assert settlement.rate == Decimal("1")
    assert settlement.gross == Decimal("100.00")
    assert settlement.fee == Decimal("5")
    assert settlement.net == Decimal("95.00")
    assert rates.requests == []


def test_unquoted_pair_raises_unknown_currency_pair():
    with pytest.raises(fxsettle.UnknownCurrencyPair):
        fxsettle.settle(invoice("100.00"), "GBP")


def test_failing_service_raises_rate_unavailable():
    with pytest.raises(fxsettle.RateUnavailable):
        fxsettle.settle(invoice("100.00"), "BRK")


def test_unset_service_url_raises_rate_unavailable(monkeypatch):
    monkeypatch.delenv("FXSETTLE_RATES_URL", raising=False)

    with pytest.raises(fxsettle.RateUnavailable):
        fxsettle.settle(invoice("100.00"), "CAD")


def test_the_two_failure_modes_are_independently_catchable():
    assert not issubclass(fxsettle.UnknownCurrencyPair, fxsettle.RateUnavailable)
    assert not issubclass(fxsettle.RateUnavailable, fxsettle.UnknownCurrencyPair)
