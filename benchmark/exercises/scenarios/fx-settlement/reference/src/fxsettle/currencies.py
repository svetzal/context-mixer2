"""Static reference data for settlement currencies."""

CURRENCY_NAMES = {
    "CAD": "Canadian dollar",
    "EUR": "Euro",
    "GBP": "Pound sterling",
    "JPY": "Japanese yen",
    "KRW": "South Korean won",
    "USD": "United States dollar",
}


def currency_name(code):
    """Return the display name for an ISO 4217 code, or the code itself."""
    return CURRENCY_NAMES.get(code, code)
