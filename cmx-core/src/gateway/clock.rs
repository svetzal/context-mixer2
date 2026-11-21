//! The [`Clock`] gateway trait — wall-clock time as an injectable dependency.

use jiff::Timestamp;

/// Abstraction over wall-clock time.
///
/// Allows tests to inject a fixed instant so that time-dependent logic
/// (staleness checks, lock file timestamps) is deterministic.
pub trait Clock {
    /// The current instant.
    fn now(&self) -> Timestamp;
}
