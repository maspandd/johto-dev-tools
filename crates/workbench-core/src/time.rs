//! Timestamps with a precision every supported database can store.

use chrono::{DateTime, DurationRound, TimeDelta, Utc};

/// Current UTC time truncated to whole microseconds.
///
/// PostgreSQL `TIMESTAMPTZ` keeps microseconds, so values created with
/// this function survive a database round trip unchanged.
#[must_use]
pub fn now() -> DateTime<Utc> {
    truncate(Utc::now())
}

/// Truncates a timestamp to whole microseconds.
#[must_use]
pub fn truncate(t: DateTime<Utc>) -> DateTime<Utc> {
    t.duration_trunc(TimeDelta::microseconds(1)).unwrap_or(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn truncates_to_microseconds() {
        let t =
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + TimeDelta::nanoseconds(1_234_567);
        assert_eq!(truncate(t).nanosecond(), 1_234_000);
        assert_eq!(now().nanosecond() % 1_000, 0);
    }
}
