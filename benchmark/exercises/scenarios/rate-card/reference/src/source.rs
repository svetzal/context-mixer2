//! The contract the core depends on, owned here rather than by the standard library.

use crate::error::QuoteError;

/// Supplies the rate card text.
///
/// The core takes this rather than a path or a file handle, so pricing can be
/// exercised without a filesystem and the transport can change without touching
/// any pricing rule.
pub trait RateCardSource {
    /// Returns the raw rate card contents.
    ///
    /// # Errors
    ///
    /// Returns [`QuoteError::RateCardUnavailable`] when the card cannot be read.
    fn load(&self) -> Result<String, QuoteError>;
}
