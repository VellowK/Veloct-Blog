//! Time abstraction.
//!
//! Release builds must be reproducible (`handoff.md` §14.9): RSS `pubDate`, sitemap `lastmod`
//! and any build stamp have to come from the Release's frozen time, not from `now()`. Every
//! component therefore takes a [`Clock`] instead of calling the system clock directly.

use std::sync::Arc;

use time::OffsetDateTime;

/// Source of the current time.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Current instant, always in UTC.
    fn now(&self) -> OffsetDateTime;
}

/// The real system clock. Used in the app runtime, never in a reproducible build.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// A clock frozen at one instant.
///
/// Used by tests and by historical release rebuilds, where every time-derived output must be
/// the value frozen into the Release manifest (`handoff.md` §14.6, §14.9).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    instant: OffsetDateTime,
}

impl FixedClock {
    pub fn new(instant: OffsetDateTime) -> Self {
        Self {
            instant: instant.to_offset(time::UtcOffset::UTC),
        }
    }

    /// Parse an RFC 3339 timestamp, as stored in frontmatter and release manifests.
    pub fn from_rfc3339(value: &str) -> crate::Result<Self> {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Self::new)
            .map_err(|e| {
                crate::VeloctError::Invalid(format!("invalid RFC 3339 timestamp {value:?}: {e}"))
            })
    }
}

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.instant
    }
}

impl Clock for Arc<dyn Clock> {
    fn now(&self) -> OffsetDateTime {
        (**self).now()
    }
}

/// Format an instant the way Veloct stores timestamps everywhere: RFC 3339, UTC, `Z` suffix.
pub fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .to_offset(time::UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Parse an RFC 3339 timestamp into UTC.
pub fn parse_rfc3339(value: &str) -> crate::Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|t| t.to_offset(time::UtcOffset::UTC))
        .map_err(|e| crate::VeloctError::Invalid(format!("invalid RFC 3339 timestamp {value:?}: {e}")))
}

/// Format a timestamp for use inside a release id: `20260101T120000Z`.
pub fn format_release_stamp(instant: OffsetDateTime) -> String {
    let utc = instant.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_always_returns_the_same_instant() {
        // A rebuild must not be able to observe a different "now" than the frozen Release time.
        let instant = time::macros::datetime!(2026-01-01 12:00:00 UTC);
        let clock = FixedClock::new(instant);
        let a = clock.now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(a, clock.now());
        assert_eq!(a, instant);
    }

    #[test]
    fn timestamps_round_trip_through_rfc3339() {
        let instant = time::macros::datetime!(2026-03-04 05:06:07 UTC);
        let text = format_rfc3339(instant);
        assert_eq!(text, "2026-03-04T05:06:07Z");
        assert_eq!(parse_rfc3339(&text).unwrap(), instant);
    }

    #[test]
    fn non_utc_offsets_are_normalised_to_utc() {
        let parsed = parse_rfc3339("2026-03-04T13:06:07+08:00").unwrap();
        assert_eq!(parsed, time::macros::datetime!(2026-03-04 05:06:07 UTC));
        assert_eq!(format_rfc3339(parsed), "2026-03-04T05:06:07Z");
    }

    #[test]
    fn invalid_timestamp_is_rejected_with_context() {
        let err = parse_rfc3339("yesterday").unwrap_err();
        assert!(err.to_string().contains("yesterday"));
    }

    #[test]
    fn release_stamp_is_filename_safe() {
        let instant = time::macros::datetime!(2026-01-01 12:00:00 UTC);
        assert_eq!(format_release_stamp(instant), "20260101T120000Z");
    }
}
