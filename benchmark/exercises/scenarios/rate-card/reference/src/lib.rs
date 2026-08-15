//! Shipping rate quotes from a zone rate card.
//!
//! Pricing rules live in [`pricing`] and never touch the filesystem. Reading the
//! card is a [`source::RateCardSource`], implemented for real by
//! [`file_source::FileRateCardSource`] and by an in-memory fake in the tests.

pub mod card;
pub mod error;
pub mod file_source;
pub mod model;
pub mod pricing;
pub mod source;
pub mod zones;

pub use card::{Band, RateCard};
pub use error::QuoteError;
pub use file_source::FileRateCardSource;
pub use model::{Parcel, Quote};
pub use source::RateCardSource;

/// Prices a parcel using the rate card named by the environment.
///
/// # Errors
///
/// Returns [`QuoteError::RateCardUnavailable`] when the card cannot be read,
/// [`QuoteError::MalformedRateCard`] when it does not parse,
/// [`QuoteError::UnknownZone`] when the zone is not quoted, and
/// [`QuoteError::Overweight`] when the parcel exceeds every band.
pub fn quote(parcel: &Parcel) -> Result<Quote, QuoteError> {
    quote_with(parcel, &FileRateCardSource)
}

/// Prices a parcel against an explicit rate card source.
///
/// # Errors
///
/// The same failures as [`quote`], from whichever source is supplied.
pub fn quote_with(parcel: &Parcel, source: &dyn RateCardSource) -> Result<Quote, QuoteError> {
    let card = RateCard::parse(&source.load()?)?;
    let priced = pricing::price(parcel, &card)?;

    tracing::info!(
        zone = %parcel.zone,
        weight_grams = parcel.weight_grams,
        total_cents = priced.total_cents,
        "quote issued"
    );

    Ok(priced)
}
