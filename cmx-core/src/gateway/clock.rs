use chrono::{DateTime, Utc};

/// Abstraction over wall-clock time.
///
/// Allows tests to inject a fixed instant so that time-dependent logic
/// (staleness checks, lock file timestamps) is deterministic.
pub trait Clock {
    fn now(&self) -> DateTime<Utc>;
}
