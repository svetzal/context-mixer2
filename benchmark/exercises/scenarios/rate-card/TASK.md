# Task: shipping quotes for `ratecard`

Implement quote calculation in this crate. The public contract below is fixed —
an external test suite calls exactly these names. Everything else, including
internal structure, is your decision.

## Public contract

Exported from the crate root:

```rust
pub struct Parcel { pub weight_grams: u32, pub zone: String }

pub struct Quote {
    pub base_cents: u64,
    pub surcharge_cents: u64,
    pub total_cents: u64,
    pub band_max_grams: u32,
}

pub enum QuoteError {
    RateCardUnavailable,
    MalformedRateCard { line: usize },
    UnknownZone(String),
    Overweight { grams: u32, limit: u32 },
}

pub fn quote(parcel: &Parcel) -> Result<Quote, QuoteError>;
```

`Quote` and `Parcel` fields are read directly. `QuoteError` variants are matched
directly. `QuoteError` must implement `std::fmt::Display` and
`std::error::Error`.

## The rate card

`quote` reads a rate card from the path in the `RATECARD_PATH` environment
variable. The file is tab-separated, one band per line. Lines that are empty or
begin with `#` are ignored.

```text
# zone	band_max_grams	cents
domestic	500	599
domestic	2000	899
domestic	20000	1799
international	500	1499
international	20000	4999
```

## Behaviour

1. Bands for a zone apply in ascending `band_max_grams`. The matching band is
   the first whose `band_max_grams` is greater than or equal to the parcel
   weight. `base_cents` is that band's cents, and `band_max_grams` is its
   threshold.
2. Surcharges add together:
   - weight strictly above 10000 grams adds 500 cents
   - zone `international` adds 20% of `base_cents`, rounded half-up to the cent
3. `total_cents` is `base_cents + surcharge_cents`.
4. A zone absent from the rate card is `UnknownZone` carrying the requested
   zone.
5. A weight above the zone's largest band is `Overweight`, where `limit` is that
   largest `band_max_grams`.
6. A missing, unreadable, or unset `RATECARD_PATH` is `RateCardUnavailable`.
7. A line that is not three tab-separated fields, or whose numbers do not parse,
   is `MalformedRateCard` carrying the 1-based line number of the offending
   line, counting every line in the file including comments and blanks.

## Observability

Each quote must be observable in operation: emit a diagnostic record per call
carrying the zone, the weight in grams, and the resulting total in cents.

## Definition of done

- The behaviour above is implemented.
- The crate carries its own tests and they pass.
- `cargo test` exits zero from the crate root.
