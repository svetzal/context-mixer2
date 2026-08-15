//! The functional core: band selection and surcharge arithmetic.
//!
//! Nothing here reads the environment, the filesystem, or the clock. Every
//! function returns the same answer for the same arguments, which is why the
//! unit tests below need no fixtures at all.

use crate::card::RateCard;
use crate::error::QuoteError;
use crate::model::{Parcel, Quote};

/// Weight above which a parcel attracts flat handling, in grams.
pub const HEAVY_THRESHOLD_GRAMS: u32 = 10_000;

/// The flat handling surcharge for a heavy parcel, in cents.
pub const HEAVY_SURCHARGE_CENTS: u64 = 500;

/// The international uplift, as a percentage of the band price.
pub const INTERNATIONAL_SURCHARGE_PERCENT: u64 = 20;

/// The zone that attracts the international uplift.
pub const INTERNATIONAL_ZONE: &str = "international";

/// Prices a parcel against an already-loaded rate card.
///
/// # Errors
///
/// Returns [`QuoteError::UnknownZone`] when the card quotes no bands for the
/// parcel's zone, and [`QuoteError::Overweight`] when the parcel exceeds the
/// heaviest band that zone offers.
pub fn price(parcel: &Parcel, card: &RateCard) -> Result<Quote, QuoteError> {
    let bands = card.bands(&parcel.zone);
    let Some(heaviest) = bands.last() else {
        return Err(QuoteError::UnknownZone(parcel.zone.clone()));
    };
    let Some(band) = bands.iter().find(|band| band.max_grams >= parcel.weight_grams) else {
        return Err(QuoteError::Overweight {
            grams: parcel.weight_grams,
            limit: heaviest.max_grams,
        });
    };

    let base_cents = band.cents;
    let surcharge_cents = surcharge(parcel, base_cents);
    Ok(Quote {
        base_cents,
        surcharge_cents,
        total_cents: base_cents + surcharge_cents,
        band_max_grams: band.max_grams,
    })
}

/// Returns everything added on top of a band price.
pub fn surcharge(parcel: &Parcel, base_cents: u64) -> u64 {
    let mut total = 0;
    if parcel.weight_grams > HEAVY_THRESHOLD_GRAMS {
        total += HEAVY_SURCHARGE_CENTS;
    }
    if parcel.zone == INTERNATIONAL_ZONE {
        total += percent_half_up(base_cents, INTERNATIONAL_SURCHARGE_PERCENT);
    }
    total
}

/// Takes a percentage of an amount, rounding halves up, in integer cents.
pub fn percent_half_up(amount_cents: u64, percent: u64) -> u64 {
    (amount_cents * percent + 50) / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> RateCard {
        RateCard::parse(
            "domestic\t500\t599\ndomestic\t20000\t1799\ninternational\t500\t1499\n",
        )
        .unwrap()
    }

    fn parcel(weight_grams: u32, zone: &str) -> Parcel {
        Parcel {
            weight_grams,
            zone: zone.to_string(),
        }
    }

    #[test]
    fn picks_the_first_band_that_covers_the_weight() {
        let quote = price(&parcel(600, "domestic"), &card()).unwrap();

        assert_eq!(quote.band_max_grams, 20000);
        assert_eq!(quote.base_cents, 1799);
    }

    #[test]
    fn treats_a_band_bound_as_inclusive() {
        let quote = price(&parcel(500, "domestic"), &card()).unwrap();

        assert_eq!(quote.band_max_grams, 500);
    }

    #[test]
    fn rounds_a_percentage_surcharge_half_up() {
        assert_eq!(percent_half_up(1499, 20), 300);
        assert_eq!(percent_half_up(4999, 20), 1000);
    }

    #[test]
    fn applies_flat_handling_only_above_the_threshold() {
        assert_eq!(surcharge(&parcel(HEAVY_THRESHOLD_GRAMS, "domestic"), 1799), 0);
        assert_eq!(
            surcharge(&parcel(HEAVY_THRESHOLD_GRAMS + 1, "domestic"), 1799),
            HEAVY_SURCHARGE_CENTS
        );
    }

    #[test]
    fn names_a_zone_the_card_does_not_quote() {
        let error = price(&parcel(400, "moon"), &card()).unwrap_err();

        assert_eq!(error, QuoteError::UnknownZone("moon".to_string()));
    }

    #[test]
    fn reports_the_heaviest_band_when_a_parcel_exceeds_it() {
        let error = price(&parcel(25000, "domestic"), &card()).unwrap_err();

        assert_eq!(
            error,
            QuoteError::Overweight {
                grams: 25000,
                limit: 20000
            }
        );
    }
}
