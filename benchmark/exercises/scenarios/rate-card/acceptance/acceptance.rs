//! Hidden acceptance checks for the rate-card exercise.
//!
//! Staged into the crate only after adherence has been scored, because a Rust
//! integration test has to live inside the crate to run at all and would
//! otherwise count as the agent's own test layer.
//!
//! Uses nothing but `std`, so it cannot be broken by whatever the agent did or
//! did not put in `[dev-dependencies]`. The runner passes `--test-threads=1`
//! because `RATECARD_PATH` is process-global state.

// The crate under test may deny `unsafe_code`, and `set_var` is unsafe from
// edition 2024 on. An in-source allow overrides a command-line `-D`, so the
// hidden suite stays runnable whatever lint policy the agent adopted.
#![allow(unsafe_code, unused_unsafe)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratecard::{quote, Parcel, QuoteError};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const CARD: &str = "# zone\tband_max_grams\tcents\n\
domestic\t500\t599\n\
domestic\t2000\t899\n\
domestic\t20000\t1799\n\
international\t500\t1499\n\
international\t20000\t4999\n";

// `set_var` is safe in edition 2021 and unsafe in 2024. The `unsafe` block
// compiles under both; the allow keeps the 2021 warning from tripping a crate
// that denies warnings.
fn install(contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "ratecard-acceptance-{}-{}.tsv",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::write(&path, contents).expect("write the rate card");
    unsafe { std::env::set_var("RATECARD_PATH", &path) };
    path
}

fn clear_rate_card() {
    unsafe { std::env::remove_var("RATECARD_PATH") };
}

/// Take the error without requiring `Quote` to implement `Debug`.
fn error_from(outcome: Result<ratecard::Quote, QuoteError>) -> QuoteError {
    match outcome {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

fn parcel(weight_grams: u32, zone: &str) -> Parcel {
    Parcel {
        weight_grams,
        zone: zone.to_string(),
    }
}

#[test]
fn lightest_domestic_band_carries_no_surcharge() {
    install(CARD);

    let result = quote(&parcel(400, "domestic")).expect("a quote");

    assert_eq!(result.band_max_grams, 500);
    assert_eq!(result.base_cents, 599);
    assert_eq!(result.surcharge_cents, 0);
    assert_eq!(result.total_cents, 599);
}

#[test]
fn weight_selects_the_first_band_that_covers_it() {
    install(CARD);

    let result = quote(&parcel(1500, "domestic")).expect("a quote");

    assert_eq!(result.band_max_grams, 2000);
    assert_eq!(result.total_cents, 899);
}

#[test]
fn heavy_parcels_add_a_flat_handling_surcharge() {
    install(CARD);

    let result = quote(&parcel(15000, "domestic")).expect("a quote");

    assert_eq!(result.base_cents, 1799);
    assert_eq!(result.surcharge_cents, 500);
    assert_eq!(result.total_cents, 2299);
}

#[test]
fn international_adds_a_percentage_rounded_half_up() {
    install(CARD);

    // 20% of 1499 is 299.8, which is 300 only under half-up rounding.
    let result = quote(&parcel(400, "international")).expect("a quote");

    assert_eq!(result.base_cents, 1499);
    assert_eq!(result.surcharge_cents, 300);
    assert_eq!(result.total_cents, 1799);
}

#[test]
fn surcharges_accumulate() {
    install(CARD);

    // 20% of 4999 is 999.8 which rounds to 1000, plus 500 for the weight.
    let result = quote(&parcel(15000, "international")).expect("a quote");

    assert_eq!(result.surcharge_cents, 1500);
    assert_eq!(result.total_cents, 6499);
}

#[test]
fn boundaries_are_inclusive_for_bands_and_exclusive_for_the_heavy_surcharge() {
    install(CARD);

    let exactly_a_band = quote(&parcel(500, "domestic")).expect("a quote");
    assert_eq!(exactly_a_band.band_max_grams, 500);
    assert_eq!(exactly_a_band.base_cents, 599);

    let exactly_the_threshold = quote(&parcel(10000, "domestic")).expect("a quote");
    assert_eq!(exactly_the_threshold.surcharge_cents, 0);
    assert_eq!(exactly_the_threshold.total_cents, 1799);
}

#[test]
fn an_unlisted_zone_names_itself() {
    install(CARD);

    let error = error_from(quote(&parcel(400, "moon")));

    match error {
        QuoteError::UnknownZone(zone) => assert_eq!(zone, "moon"),
        other => panic!("expected UnknownZone, got {other:?}"),
    }
}

#[test]
fn a_parcel_beyond_every_band_reports_the_limit() {
    install(CARD);

    let error = error_from(quote(&parcel(25000, "domestic")));

    match error {
        QuoteError::Overweight { grams, limit } => {
            assert_eq!(grams, 25000);
            assert_eq!(limit, 20000);
        }
        other => panic!("expected Overweight, got {other:?}"),
    }
}

#[test]
fn a_missing_rate_card_is_unavailable_rather_than_a_panic() {
    clear_rate_card();

    let error = error_from(quote(&parcel(400, "domestic")));

    assert!(matches!(error, QuoteError::RateCardUnavailable));
}

#[test]
fn a_malformed_line_reports_its_own_line_number() {
    // Line 1 is a comment and line 3 is blank; both still count.
    install("# zone\tband_max_grams\tcents\ndomestic\t500\t599\n\ndomestic\tnotanumber\t899\n");

    let error = error_from(quote(&parcel(400, "domestic")));

    match error {
        QuoteError::MalformedRateCard { line } => assert_eq!(line, 4),
        other => panic!("expected MalformedRateCard, got {other:?}"),
    }
}
