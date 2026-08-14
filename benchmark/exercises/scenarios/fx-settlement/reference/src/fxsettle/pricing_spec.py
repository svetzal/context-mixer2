from decimal import Decimal

from fxsettle.models import Invoice
from fxsettle.pricing import fee_rate, minor_units, settle_at_rate, to_currency


def an_invoice(amount, currency="USD"):
    return Invoice(amount=Decimal(amount), currency=currency, counterparty="Acme")


class DescribeMinorUnits:
    def should_settle_most_currencies_to_two_places(self):
        assert minor_units("CAD") == 2

    def should_settle_yen_and_won_to_whole_units(self):
        assert minor_units("JPY") == 0
        assert minor_units("KRW") == 0


class DescribeCurrencyRounding:
    def should_round_halves_up_rather_than_to_even(self):
        assert to_currency(Decimal("101.565"), "CAD") == Decimal("101.57")

    def should_drop_the_fraction_for_whole_unit_currencies(self):
        assert to_currency(Decimal("1257.6"), "JPY") == Decimal("1258")


class DescribeFeeTiers:
    def should_charge_two_and_a_half_percent_up_to_a_thousand(self):
        assert fee_rate(Decimal("1000")) == Decimal("0.025")

    def should_charge_one_and_a_half_percent_up_to_ten_thousand(self):
        assert fee_rate(Decimal("10000")) == Decimal("0.015")

    def should_charge_zero_point_eight_percent_above_ten_thousand(self):
        assert fee_rate(Decimal("10000.01")) == Decimal("0.008")


class DescribeSettlingAtAKnownRate:
    def should_convert_then_charge_the_tier_the_converted_amount_falls_in(self):
        settlement = settle_at_rate(an_invoice("500.00"), "CAD", Decimal("1.3542"))

        assert settlement.gross == Decimal("677.10")
        assert settlement.fee == Decimal("16.93")
        assert settlement.net == Decimal("660.17")

    def should_never_charge_less_than_five_units(self):
        settlement = settle_at_rate(an_invoice("10.00"), "CAD", Decimal("1.3542"))

        assert settlement.fee == Decimal("5.00")

    def should_carry_both_currencies_into_the_result(self):
        settlement = settle_at_rate(an_invoice("100.00"), "CAD", Decimal("1.3542"))

        assert settlement.base_currency == "USD"
        assert settlement.quote_currency == "CAD"
