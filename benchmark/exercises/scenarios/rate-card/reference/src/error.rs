//! The failures a caller is expected to distinguish.

use std::fmt;

/// Why a parcel could not be priced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteError {
    /// The rate card could not be located or read.
    RateCardUnavailable,
    /// A rate card line did not parse, identified by its 1-based number.
    MalformedRateCard {
        /// The offending line, counting comments and blanks.
        line: usize,
    },
    /// The rate card quotes no bands for the requested zone.
    UnknownZone(String),
    /// The parcel is heavier than the zone's largest band.
    Overweight {
        /// The parcel's weight.
        grams: u32,
        /// The heaviest weight the zone will carry.
        limit: u32,
    },
}

impl fmt::Display for QuoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateCardUnavailable => write!(formatter, "the rate card could not be read"),
            Self::MalformedRateCard { line } => {
                write!(formatter, "the rate card is malformed at line {line}")
            }
            Self::UnknownZone(zone) => write!(formatter, "no bands are quoted for zone {zone}"),
            Self::Overweight { grams, limit } => write!(
                formatter,
                "{grams}g exceeds the heaviest band of {limit}g for this zone"
            ),
        }
    }
}

impl std::error::Error for QuoteError {}
