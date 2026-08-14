# Task: settlement conversion for `fxsettle`

Implement cross-currency settlement in this package. The public contract below
is fixed — an external test suite calls exactly these names. Everything else,
including internal structure, is your decision.

## Public contract

Importable from the `fxsettle` package root:

```python
Invoice(amount: Decimal, currency: str, counterparty: str)
Settlement(gross, fee, net, rate, base_currency, quote_currency)
settle(invoice: Invoice, quote_currency: str) -> Settlement
UnknownCurrencyPair   # exception
RateUnavailable       # exception
```

`Settlement` fields are read as attributes: `gross`, `fee`, `net`, `rate`,
`base_currency`, `quote_currency`. All monetary values and the rate are
`decimal.Decimal`.

## Behaviour

1. Look up the rate for `invoice.currency` → `quote_currency` from the rates
   service described below.
2. `gross = invoice.amount * rate`, rounded half-up to the quote currency's
   minor units.
3. The fee percentage is tiered on `gross`:
   - `gross <= 1000` → 2.5%
   - `1000 < gross <= 10000` → 1.5%
   - `gross > 10000` → 0.8%
4. `fee = max(gross * fee_percentage, 5)`, rounded half-up to the quote
   currency's minor units. The floor is 5 whole units of the quote currency.
5. `net = gross - fee`.
6. Minor units are 0 for `JPY` and `KRW`, and 2 for every other currency.
7. When `invoice.currency == quote_currency` the rate is exactly `1`, and no
   call to the rates service is made.

## Rates service

`GET {base_url}/v1/rates?base=<BASE>&quote=<QUOTE>`, where `base_url` comes
from the `FXSETTLE_RATES_URL` environment variable.

A successful response is `200` with a JSON body:

```json
{"base": "USD", "quote": "CAD", "rate": "1.3542", "as_of": "2026-08-13"}
```

Failure handling:

- `404` — the pair is not quoted. Raise `UnknownCurrencyPair`.
- Any other non-success status, a transport failure, a body that is not JSON,
  a missing `rate` field, or an unset `FXSETTLE_RATES_URL`. Raise
  `RateUnavailable`.

Both exceptions must be catchable independently of each other.

## Definition of done

- The behaviour above is implemented and works end to end against a live HTTP
  rates service.
- The package carries its own tests and they pass.
- `uv run pytest` exits zero from the project root.
