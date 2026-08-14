# fxsettle

Cross-currency invoice settlement for the accounts-receivable service.

Invoices are raised in a counterparty's local currency and settled into the
ledger currency of the receiving entity. This package owns the conversion, the
tiered settlement fee, and the currency rounding rules.

## Development

```bash
uv sync
uv run pytest
```
