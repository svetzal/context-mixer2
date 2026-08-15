//! The effect boundary: the one place this crate touches the filesystem.

use std::env;
use std::fs;

use crate::error::QuoteError;
use crate::source::RateCardSource;

/// The environment variable naming the rate card.
pub const RATE_CARD_PATH_VARIABLE: &str = "RATECARD_PATH";

/// Reads the rate card from the path in the environment.
#[derive(Debug, Default, Clone, Copy)]
pub struct FileRateCardSource;

impl RateCardSource for FileRateCardSource {
    fn load(&self) -> Result<String, QuoteError> {
        let path = env::var(RATE_CARD_PATH_VARIABLE).map_err(|_| QuoteError::RateCardUnavailable)?;
        fs::read_to_string(path).map_err(|_| QuoteError::RateCardUnavailable)
    }
}
