//! Cross-module wiring, exercised through an in-memory rate card source.

use ratecard::{quote_with, Parcel, QuoteError, RateCardSource};

/// A rate card held in memory, so wiring tests never touch a filesystem.
struct StaticRateCard(&'static str);

impl RateCardSource for StaticRateCard {
    fn load(&self) -> Result<String, QuoteError> {
        Ok(self.0.to_string())
    }
}

/// A source that always fails, standing in for a missing card.
struct UnavailableRateCard;

impl RateCardSource for UnavailableRateCard {
    fn load(&self) -> Result<String, QuoteError> {
        Err(QuoteError::RateCardUnavailable)
    }
}

const CARD: &str = "# zone\tband_max_grams\tcents\n\
domestic\t500\t599\n\
domestic\t20000\t1799\n\
international\t500\t1499\n";

fn parcel(weight_grams: u32, zone: &str) -> Parcel {
    Parcel {
        weight_grams,
        zone: zone.to_string(),
    }
}

#[test]
fn prices_a_parcel_end_to_end_from_a_card() {
    let quote = quote_with(&parcel(400, "domestic"), &StaticRateCard(CARD)).unwrap();

    assert_eq!(quote.base_cents, 599);
    assert_eq!(quote.total_cents, 599);
}

#[test]
fn combines_the_percentage_and_handling_surcharges() {
    let quote = quote_with(&parcel(15000, "international"), &StaticRateCard(
        "international\t20000\t4999\n",
    ))
    .unwrap();

    assert_eq!(quote.surcharge_cents, 1500);
    assert_eq!(quote.total_cents, 6499);
}

#[test]
fn surfaces_an_unreadable_card_to_the_caller() {
    let error = quote_with(&parcel(400, "domestic"), &UnavailableRateCard).unwrap_err();

    assert_eq!(error, QuoteError::RateCardUnavailable);
}

#[test]
fn surfaces_a_malformed_card_with_its_line_number() {
    let error = quote_with(&parcel(400, "domestic"), &StaticRateCard("domestic\tx\t1\n")).unwrap_err();

    assert_eq!(error, QuoteError::MalformedRateCard { line: 1 });
}
