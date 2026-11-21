//! The RFC 3339 form cmx writes for every timestamp it records.
//!
//! Lock files (`installed_at`), `sources.json` (`last_updated`) and compile
//! manifests (`compiled_at`) all carry timestamps written by this module, so its
//! output is an on-disk format. It keeps the shape earlier releases wrote: a
//! `+00:00` offset rather than `Z`, and fractional seconds only when the instant
//! has them, as 3, 6 or 9 digits.

use jiff::Timestamp;
use jiff::fmt::temporal::DateTimePrinter;
use jiff::tz::Offset;

/// Format `timestamp` as RFC 3339 in UTC, e.g. `2026-07-05T12:00:00+00:00`.
///
/// Sub-second precision is the shortest of 0, 3, 6 or 9 digits that loses
/// nothing: `12:00:00.250+00:00`, `12:00:00.000001+00:00`.
///
/// ```
/// use cmx_core::timestamp::to_rfc3339;
///
/// let t: jiff::Timestamp = "2026-07-05T12:00:00Z".parse().unwrap();
/// assert_eq!(to_rfc3339(t), "2026-07-05T12:00:00+00:00");
/// ```
pub fn to_rfc3339(timestamp: Timestamp) -> String {
    DateTimePrinter::new()
        .precision(Some(fraction_digits(timestamp.subsec_nanosecond())))
        .timestamp_with_offset_to_string(&timestamp, Offset::UTC)
}

/// The fewest of 0, 3, 6 or 9 digits that represent `nanos` exactly.
fn fraction_digits(nanos: i32) -> u8 {
    if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn whole_seconds_have_no_fraction_and_a_numeric_utc_offset() {
        assert_eq!(to_rfc3339(ts("2024-06-01T12:00:00Z")), "2024-06-01T12:00:00+00:00");
    }

    #[test]
    fn fractions_use_millisecond_microsecond_or_nanosecond_groups() {
        assert_eq!(to_rfc3339(ts("2024-06-01T12:00:00.25Z")), "2024-06-01T12:00:00.250+00:00");
        assert_eq!(
            to_rfc3339(ts("2024-06-01T12:00:00.000001Z")),
            "2024-06-01T12:00:00.000001+00:00"
        );
        assert_eq!(
            to_rfc3339(ts("2024-06-01T12:00:00.1234567Z")),
            "2024-06-01T12:00:00.123456700+00:00"
        );
    }

    #[test]
    fn pre_epoch_instants_format_like_any_other() {
        assert_eq!(to_rfc3339(ts("1969-12-31T23:59:59.5Z")), "1969-12-31T23:59:59.500+00:00");
    }

    /// Strings in the shape earlier releases (built on chrono's `to_rfc3339`)
    /// wrote to lock files, `sources.json` and compile manifests. Each must read
    /// back to the same instant and be written again byte for byte.
    #[test]
    fn previously_written_timestamps_round_trip_unchanged() {
        for written in [
            "2026-07-05T12:00:00+00:00",
            "2026-09-05T14:02:11+00:00",
            "2026-11-18T09:15:42.123+00:00",
            "2026-11-18T09:15:42.123456+00:00",
            "2026-11-18T09:15:42.123456789+00:00",
        ] {
            let parsed: Timestamp = written.parse().unwrap();
            assert_eq!(to_rfc3339(parsed), written);
        }
    }

    #[test]
    fn non_utc_offsets_and_z_suffixes_read_as_the_same_instant() {
        assert_eq!(ts("2026-07-05T14:00:00+02:00"), ts("2026-07-05T12:00:00+00:00"));
        assert_eq!(ts("2026-07-05T12:00:00Z"), ts("2026-07-05T12:00:00+00:00"));
    }
}
