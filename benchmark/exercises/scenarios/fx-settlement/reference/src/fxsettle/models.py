"""Immutable domain values exchanged across the settlement boundary."""

from decimal import Decimal

from pydantic import BaseModel, ConfigDict


class Invoice(BaseModel):
    """An amount owed by a counterparty in their own currency."""

    model_config = ConfigDict(frozen=True)

    amount: Decimal
    currency: str
    counterparty: str


class Settlement(BaseModel):
    """The result of converting an invoice into a ledger currency."""

    model_config = ConfigDict(frozen=True)

    gross: Decimal
    fee: Decimal
    net: Decimal
    rate: Decimal
    base_currency: str
    quote_currency: str
