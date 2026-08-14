from decimal import Decimal

import pytest
from pydantic import ValidationError

from fxsettle.models import Invoice, Settlement


class DescribeInvoice:
    def should_reject_assignment_after_construction(self):
        invoice = Invoice(amount=Decimal("100.00"), currency="USD", counterparty="Acme")

        with pytest.raises(ValidationError):
            invoice.amount = Decimal("1.00")


class DescribeSettlement:
    def should_reject_assignment_after_construction(self):
        settlement = Settlement(
            gross=Decimal("100.00"),
            fee=Decimal("5.00"),
            net=Decimal("95.00"),
            rate=Decimal("1"),
            base_currency="USD",
            quote_currency="USD",
        )

        with pytest.raises(ValidationError):
            settlement.net = Decimal("0.00")
