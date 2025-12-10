//! RFC 5322 Date header formatting.
//!
//! Provides typed time formatting for email Date headers using chrono.
//!
//! Works in `no_std` and `no_alloc` environments. We manually format RFC 2822 dates
//! without allocation, so chrono's `alloc` feature is not required.

use core::fmt;

use chrono::{DateTime as ChronoDateTime, FixedOffset, TimeZone as ChronoTimeZone, Utc};

/// A timezone offset from UTC.
///
/// Represents a fixed timezone offset (e.g., UTC+5:30, UTC-8:00).
/// Does not handle daylight saving time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeZone {
    offset_minutes: Option<i32>,
}

impl TimeZone {
    /// Create a timezone offset ahead of UTC (east of UTC).
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::TimeZone;
    ///
    /// let ist = TimeZone::plus(5, 30).unwrap(); // UTC+5:30 (India)
    /// let cet = TimeZone::plus(1, 0).unwrap();   // UTC+1 (Central European Time)
    /// ```
    #[must_use]
    pub fn plus(hours: u32, minutes: u32) -> Option<Self> {
        if hours > 12 || minutes > 59 {
            return None;
        }
        let offset_minutes = Some((hours * 60 + minutes) as i32);
        Some(TimeZone { offset_minutes })
    }

    /// Create a timezone offset behind UTC (west of UTC).
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::TimeZone;
    ///
    /// let est = TimeZone::minus(5, 0).unwrap();  // UTC-5 (Eastern Standard Time)
    /// let pst = TimeZone::minus(8, 0).unwrap();  // UTC-8 (Pacific Standard Time)
    /// ```
    #[must_use]
    pub fn minus(hours: u32, minutes: u32) -> Option<Self> {
        if hours > 12 || minutes > 59 {
            return None;
        }
        let offset_minutes = -((hours * 60 + minutes) as i32);
        Some(TimeZone {
            offset_minutes: if offset_minutes == 0 {
                // -00:00 is undefined
                None
            } else {
                Some(offset_minutes)
            },
        })
    }

    /// UTC timezone (offset +0000).
    #[must_use]
    pub fn utc() -> Self {
        TimeZone {
            offset_minutes: Some(0),
        }
    }

    /// Undefined timezone (offset -0000).
    ///
    /// Per RFC 5322 §3.3, `-0000` indicates that the time was generated on a system
    /// that may be in a local time zone other than UTC and that the date-time contains
    /// no information about the local time zone. This is distinct from `+0000`, which
    /// explicitly indicates UTC.
    ///
    /// For calculations, `-0000` is treated as UTC (same as `+0000`), but it signifies
    /// that the original time zone is unknown.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::TimeZone;
    ///
    /// let undefined = TimeZone::undefined();
    /// // Displays as -0000 in formatted date strings
    /// ```
    ///
    /// **References:**
    /// - [RFC 5322 Section 3.3 - Date and Time](https://datatracker.ietf.org/doc/html/rfc5322#section-3.3)
    #[must_use]
    pub fn undefined() -> Self {
        TimeZone {
            offset_minutes: None,
        }
    }

    /// Get the offset in seconds from UTC.
    #[must_use]
    fn offset_seconds(&self) -> i32 {
        self.offset_minutes.unwrap_or_default() * 60
    }

    /// Get the offset in minutes from UTC.
    #[must_use]
    fn offset_minutes(&self) -> Option<i32> {
        self.offset_minutes
    }
}

/// A date-time value for the Date header, per RFC 5322 §3.3.
///
/// Wraps chrono's DateTime for proper RFC 2822 formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    utc: ChronoDateTime<Utc>,
    zone: TimeZone,
}

impl DateTime {
    /// Create a date-time from UTC components.
    ///
    /// The time is interpreted as being in UTC.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::DateTime;
    ///
    /// let date = DateTime::from_utc(2025, 12, 7, 12, 0, 0).unwrap();
    /// ```
    #[must_use]
    pub fn from_utc(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Option<Self> {
        let offset = FixedOffset::east_opt(0)?;
        offset
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .map(|dt| DateTime {
                utc: dt.with_timezone(&Utc),
                zone: TimeZone::utc(),
            })
    }

    /// Create a date-time from local time components in the given zone.
    ///
    /// The time is interpreted as being in the specified zone.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::{DateTime, TimeZone};
    ///
    /// // 14:30 in UTC+1 (CET)
    /// let date = DateTime::from_local(
    ///     2025, 12, 7, 14, 30, 0,
    ///     TimeZone::plus(1, 0).unwrap()
    /// ).unwrap();
    /// // Displays as "14:30:00 +0100"
    /// ```
    #[must_use]
    pub fn from_local(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        zone: TimeZone,
    ) -> Option<Self> {
        let offset = FixedOffset::east_opt(zone.offset_seconds())?;
        offset
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .map(|dt| DateTime {
                utc: dt.with_timezone(&Utc),
                zone,
            })
    }

    /// Create a date from a Unix timestamp (seconds since 1970-01-01 00:00:00 UTC).
    /// always returns a UTC time
    #[must_use]
    pub fn from_timestamp(secs: i64) -> Option<Self> {
        ChronoDateTime::<Utc>::from_timestamp(secs, 0).map(|dt: ChronoDateTime<Utc>| DateTime {
            utc: dt,
            zone: TimeZone::utc(),
        })
    }

    /// Create a date from a Unix timestamp with milliseconds.
    /// always returns a UTC time
    #[must_use]
    pub fn from_timestamp_millis(millis: i64) -> Option<Self> {
        ChronoDateTime::<Utc>::from_timestamp_millis(millis).map(|dt: ChronoDateTime<Utc>| {
            DateTime {
                utc: dt,
                zone: TimeZone::utc(),
            }
        })
    }

    /// Convert to a different timezone while keeping the same point in time.
    ///
    /// This converts the actual time value to the new timezone.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::{DateTime, TimeZone};
    ///
    /// // 12:00 UTC converted to UTC+1 should show 13:00
    /// let utc = DateTime::from_utc(2025, 12, 7, 12, 0, 0).unwrap();
    /// let cet = utc.to_zone(TimeZone::plus(1, 0).unwrap()).unwrap();
    ///
    /// assert!(cet.to_string().contains("13:00:00 +0100"));
    /// ```
    #[must_use]
    pub fn to_zone(self, zone: TimeZone) -> Option<Self> {
        // Keep the same UTC time, just change the display timezone
        Some(DateTime {
            utc: self.utc,
            zone,
        })
    }

    /// Get the current UTC time as a DateTime.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn now_utc() -> Self {
        DateTime {
            utc: Utc::now(),
            zone: TimeZone::utc(),
        }
    }

    /// Get the current local time in the given zone as a DateTime.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::{DateTime, TimeZone};
    ///
    /// let now = DateTime::now_local(TimeZone::plus(1, 0).unwrap());
    /// ```
    #[cfg(feature = "std")]
    #[must_use]
    pub fn now_local(zone: TimeZone) -> Self {
        DateTime {
            utc: Utc::now(),
            zone,
        }
    }
}

impl fmt::Display for DateTime {
    /// Formats the date-time according to RFC 5322 §3.3.
    ///
    /// **Format:** `day-of-week, day month year hour:minute:second zone`
    ///
    /// **Components:**
    /// - `day-of-week`: Three-letter abbreviation (Mon, Tue, Wed, Thu, Fri, Sat, Sun)
    /// - `day`: 1-2 digit day of month (01-31)
    /// - `month`: Three-letter abbreviation (Jan, Feb, Mar, Apr, May, Jun, Jul, Aug, Sep, Oct, Nov, Dec)
    /// - `year`: 4-digit year
    /// - `hour:minute:second`: 24-hour time format (00:00:00 to 23:59:59)
    /// - `zone`: Timezone offset as `+HHMM` or `-HHMM` (4 digits with sign)
    ///
    /// **Example:** `Sun, 7 Dec 2025 12:30:00 +0100`
    ///
    /// **Timezone Format:**
    /// - Positive offset (`+HHMM`) means local time is ahead of UTC (east of UTC)
    /// - Negative offset (`-HHMM`) means local time is behind UTC (west of UTC)
    /// - First two digits are hours, last two digits are minutes
    /// - Examples: `+0000` (UTC), `+0100` (UTC+1), `-0500` (UTC-5), `+0530` (UTC+5:30)
    ///
    /// **References:**
    /// - [RFC 5322 Section 3.3 - Date and Time](https://datatracker.ietf.org/doc/html/rfc5322#section-3.3)
    ///
    /// This implementation manually formats the date-time to avoid requiring the `alloc` feature,
    /// making it suitable for `no_std` and `no_alloc` environments.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use chrono::{Datelike, Timelike};

        // Convert UTC time to the target timezone for display
        let offset_seconds = self.zone.offset_seconds();
        let offset = FixedOffset::east_opt(offset_seconds).ok_or(fmt::Error)?;
        let dt = self.utc.with_timezone(&offset);

        let date = dt.date_naive();
        let time = dt.time();

        // Day of week abbreviations
        let weekday = match date.weekday() {
            chrono::Weekday::Mon => "Mon",
            chrono::Weekday::Tue => "Tue",
            chrono::Weekday::Wed => "Wed",
            chrono::Weekday::Thu => "Thu",
            chrono::Weekday::Fri => "Fri",
            chrono::Weekday::Sat => "Sat",
            chrono::Weekday::Sun => "Sun",
        };

        // Month abbreviations
        let month = match date.month() {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => unreachable!(),
        };

        // Format offset: +HHMM or -HHMM
        // If offset_minutes is None, display -00:00 (undefined timezone per RFC 5322)
        let (offset_sign, offset_hours, offset_minutes) = match self.zone.offset_minutes() {
            None => ('-', 0, 0),
            Some(minutes) => {
                let abs_minutes = minutes.abs();
                let hours = (abs_minutes / 60) as u32;
                let mins = (abs_minutes % 60) as u32;
                let sign = if minutes >= 0 { '+' } else { '-' };
                (sign, hours, mins)
            }
        };

        write!(
            f,
            "{}, {:02} {} {:04} {:02}:{:02}:{:02} {}{:02}{:02}",
            weekday,
            date.day(),
            month,
            date.year(),
            time.hour(),
            time.minute(),
            time.second(),
            offset_sign,
            offset_hours,
            offset_minutes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formatting() {
        // Test basic RFC 5322 formatting
        let d =
            DateTime::from_local(2025, 12, 7, 12, 30, 0, TimeZone::plus(1, 0).unwrap()).unwrap();
        assert!(d.to_string().contains("Dec 2025"));
        assert!(d.to_string().contains("12:30:00"));
        assert!(d.to_string().contains("+0100"));

        // Test UTC formatting
        let utc = DateTime::from_utc(2025, 12, 7, 12, 0, 0).unwrap();
        assert!(utc.to_string().contains("+0000"));
    }

    #[test]
    fn date_invalid_returns_none() {
        assert!(DateTime::from_utc(2025, 13, 1, 0, 0, 0).is_none());
        assert!(DateTime::from_utc(2025, 2, 30, 0, 0, 0).is_none());
    }

    #[test]
    fn from_local_creates_time_in_zone() {
        // from_local creates a time that is interpreted as being in that zone
        let date =
            DateTime::from_local(2025, 12, 7, 12, 0, 0, TimeZone::plus(5, 0).unwrap()).unwrap();
        assert!(date.to_string().contains("12:00:00 +0500"));
    }

    #[test]
    fn date_from_timestamp() {
        // Test Unix timestamp conversion (always returns UTC)
        let d = DateTime::from_timestamp(1735689600).unwrap();
        assert!(d.to_string().contains("Jan 2025"));
        assert!(d.to_string().contains("+0000"));

        // Test millisecond timestamp
        let d = DateTime::from_timestamp_millis(1_735_689_600_123).unwrap();
        assert!(d.to_string().contains("Jan 2025"));
    }

    #[test]
    fn offset_formatting() {
        // Test various offset formats: UTC, positive/negative, with/without minutes, padding
        let test_cases: &[(u32, u32, bool, &str)] = &[
            (0, 0, true, "+0000"),   // UTC
            (1, 0, true, "+0100"),   // Positive hour only
            (5, 30, true, "+0530"),  // Positive with minutes
            (12, 0, true, "+1200"),  // Max positive
            (1, 0, false, "-0100"),  // Negative hour only
            (5, 30, false, "-0530"), // Negative with minutes
            (12, 0, false, "-1200"), // Max negative
            (9, 5, true, "+0905"),   // Single digit hour/minute (tests padding)
        ];

        for (hour, minute, is_plus, expected) in test_cases {
            let zone = if *is_plus {
                TimeZone::plus(*hour, *minute).unwrap()
            } else {
                TimeZone::minus(*hour, *minute).unwrap()
            };
            let d = DateTime::from_local(2025, 1, 1, 12, 0, 0, zone).unwrap();
            let formatted = d.to_string();
            let offset_part = &formatted[formatted.len() - 5..];
            assert_eq!(
                offset_part,
                *expected,
                "Offset mismatch for UTC{}{:02}:{:02}",
                if *is_plus { "+" } else { "-" },
                hour,
                minute
            );
        }
    }

    #[test]
    fn zone_creation() {
        // Test TimeZone::plus and TimeZone::minus create correct offsets
        let zone_plus = TimeZone::plus(1, 30).unwrap();
        let date_plus = DateTime::from_local(2025, 1, 1, 12, 0, 0, zone_plus).unwrap();
        assert!(
            date_plus.to_string().ends_with("+0130"),
            "TimeZone::plus should create positive offset"
        );

        let zone_minus = TimeZone::minus(1, 30).unwrap();
        let date_minus = DateTime::from_local(2025, 1, 1, 12, 0, 0, zone_minus).unwrap();
        assert!(
            date_minus.to_string().ends_with("-0130"),
            "TimeZone::minus should create negative offset"
        );

        let zone_utc = TimeZone::utc();
        let date_utc = DateTime::from_local(2025, 1, 1, 12, 0, 0, zone_utc).unwrap();
        assert!(
            date_utc.to_string().ends_with("+0000"),
            "TimeZone::utc should create UTC offset"
        );
    }

    #[test]
    fn to_zone_converts_time() {
        // to_zone converts the actual time to the new timezone
        let utc = DateTime::from_utc(2025, 1, 1, 12, 0, 0).unwrap();

        // Positive offset
        let cet = utc.to_zone(TimeZone::plus(1, 0).unwrap()).unwrap();
        assert!(cet.to_string().contains("13:00:00 +0100"));

        // Negative offset
        let est = utc.to_zone(TimeZone::minus(5, 0).unwrap()).unwrap();
        assert!(est.to_string().contains("07:00:00 -0500"));

        // Offset with minutes
        let ist = utc.to_zone(TimeZone::plus(5, 30).unwrap()).unwrap();
        assert!(ist.to_string().contains("17:30:00 +0530"));
    }

    #[test]
    fn from_local_vs_to_zone() {
        // Demonstrate the difference between from_local and to_zone
        // from_local: creates time in that zone (12:00 in UTC+1 = different moment than 12:00 UTC)
        let from_local =
            DateTime::from_local(2025, 1, 1, 12, 0, 0, TimeZone::plus(1, 0).unwrap()).unwrap();
        assert!(from_local.to_string().contains("12:00:00 +0100"));

        // to_zone: converts time (12:00 UTC = 13:00 in UTC+1)
        let utc = DateTime::from_utc(2025, 1, 1, 12, 0, 0).unwrap();
        let to_zone = utc.to_zone(TimeZone::plus(1, 0).unwrap()).unwrap();
        assert!(to_zone.to_string().contains("13:00:00 +0100"));

        // They represent different points in time
        assert_ne!(from_local, to_zone);
    }

    #[test]
    fn to_zone_round_trip() {
        // Converting to a timezone and back should give the original
        let original = DateTime::from_utc(2025, 1, 1, 12, 0, 0).unwrap();
        let converted = original.to_zone(TimeZone::plus(5, 0).unwrap()).unwrap();
        let back = converted.to_zone(TimeZone::utc()).unwrap();
        // Should be back to 12:00 UTC
        assert!(
            back.to_string().contains("12:00:00 +0000"),
            "Round trip should restore original time"
        );
    }

    #[test]
    fn known_timestamp_conversions() {
        // Test converting known timestamps to various timezones, including date boundary crossings
        // Format: (utc_year, utc_month, utc_day, utc_hour, utc_min, tz_hour, tz_min, expected_contains)
        #[allow(clippy::type_complexity)]
        let test_cases: &[(i32, u32, u32, u32, u32, i32, i32, &str)] = &[
            // 2025-01-01 12:00:00 UTC to various timezones
            (2025, 1, 1, 12, 0, 0, 0, "Wed, 01 Jan 2025 12:00:00 +0000"),
            (2025, 1, 1, 12, 0, -5, 0, "Wed, 01 Jan 2025 07:00:00 -0500"), // EST
            (2025, 1, 1, 12, 0, -8, 0, "Wed, 01 Jan 2025 04:00:00 -0800"), // PST
            (2025, 1, 1, 12, 0, 1, 0, "Wed, 01 Jan 2025 13:00:00 +0100"),  // CET
            (2025, 1, 1, 12, 0, 5, 30, "Wed, 01 Jan 2025 17:30:00 +0530"), // IST
            (2025, 1, 1, 12, 0, 9, 0, "Wed, 01 Jan 2025 21:00:00 +0900"),  // JST
            // Date boundary crossing: 2025-01-01 02:00:00 UTC
            (2025, 1, 1, 2, 0, -8, 0, "Tue, 31 Dec 2024 18:00:00 -0800"), // PST (previous day)
            (2025, 1, 1, 2, 0, 9, 0, "Wed, 01 Jan 2025 11:00:00 +0900"),  // JST (same day)
            // From Unix timestamp 1735732800 = 2025-01-01 12:00:00 UTC
            (2025, 1, 1, 12, 0, -5, 0, "07:00:00 -0500"), // EST
            (2025, 1, 1, 12, 0, 5, 30, "17:30:00 +0530"), // IST
        ];

        for (year, month, day, hour, min, tz_hour, tz_min, expected) in test_cases {
            let utc = DateTime::from_utc(*year, *month, *day, *hour, *min, 0).unwrap();
            let zone = if *tz_hour >= 0 {
                TimeZone::plus(*tz_hour as u32, *tz_min as u32).unwrap()
            } else {
                TimeZone::minus((-*tz_hour) as u32, *tz_min as u32).unwrap()
            };
            let converted = utc.to_zone(zone).unwrap();
            let formatted = converted.to_string();

            assert!(
                formatted.contains(*expected),
                "Conversion mismatch for {}-{:02}-{:02} {:02}:{:02} UTC to UTC{:+02}:{:02}, expected {} but got: {}",
                year,
                month,
                day,
                hour,
                min,
                tz_hour,
                tz_min,
                expected,
                formatted
            );
        }

        // Also test from_timestamp directly
        let utc = DateTime::from_timestamp(1_735_732_800).unwrap();
        assert!(utc.to_string().contains("Wed, 01 Jan 2025 12:00:00 +0000"));
    }

    #[test]
    fn none_timezone_displays_as_minus_zero() {
        // TimeZone::undefined() returns None offset_minutes (undefined timezone per RFC 5322)
        // This should display as -00:00 instead of +0000
        let undefined_zone = TimeZone::undefined();
        let date = DateTime::from_local(2025, 1, 1, 12, 0, 0, undefined_zone).unwrap();
        let formatted = date.to_string();

        // Should end with -00:00 (undefined timezone)
        assert!(
            formatted.ends_with("-0000"),
            "None timezone should display as -0000, got: {}",
            formatted
        );

        // Also test with to_zone
        let utc = DateTime::from_utc(2025, 1, 1, 12, 0, 0).unwrap();
        let converted = utc.to_zone(undefined_zone).unwrap();
        assert!(
            converted.to_string().ends_with("-0000"),
            "to_zone with None timezone should display as -0000"
        );
    }
}
