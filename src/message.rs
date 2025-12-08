//! RFC 5322 Internet Message Format implementation.
//!
//! Uses chrono for date/time handling - it's battle-tested and `no_std` compatible.

#[cfg(feature = "alloc")]
use alloc::string::String;
use core::fmt;

use chrono::{DateTime, FixedOffset, TimeZone};

// ═══════════════════════════════════════════════════════════════════════════
// TimeOffset - timezone offset representation
// ═══════════════════════════════════════════════════════════════════════════

/// A timezone offset from UTC.
///
/// Hours and minutes are always positive; the variant determines the direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOffset {
    /// Positive offset (east of UTC), e.g., UTC+5:30
    Positive { hours: u32, minutes: u32 },
    /// Negative offset (west of UTC), e.g., UTC-1:30
    Negative { hours: u32, minutes: u32 },
}

impl TimeOffset {
    /// Create a positive offset (east of UTC).
    ///
    /// Both `hours` and `minutes` must be positive.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::TimeOffset;
    ///
    /// let ist = TimeOffset::positive(5, 30); // UTC+5:30 (India)
    /// ```
    #[must_use]
    pub const fn positive(hours: u32, minutes: u32) -> Self {
        Self::Positive { hours, minutes }
    }

    /// Create a negative offset (west of UTC).
    ///
    /// Both `hours` and `minutes` must be positive.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::TimeOffset;
    ///
    /// let offset = TimeOffset::negative(1, 30); // UTC-1:30
    /// ```
    #[must_use]
    pub const fn negative(hours: u32, minutes: u32) -> Self {
        Self::Negative { hours, minutes }
    }

    /// Convert to offset seconds (positive for east, negative for west).
    fn to_seconds(self) -> i32 {
        let total_minutes = match self {
            Self::Positive { hours, minutes } => (hours * 60 + minutes) as i32,
            Self::Negative { hours, minutes } => -((hours * 60 + minutes) as i32),
        };
        total_minutes * 60
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MessageDate - wrapper around chrono's DateTime
// ═══════════════════════════════════════════════════════════════════════════

/// A date-time value for the Date header, per RFC 5322 §3.3.
///
/// Wraps chrono's DateTime for proper RFC 2822 formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageDate(DateTime<FixedOffset>);

impl MessageDate {
    /// Create a new date from components.
    ///
    /// Returns None if the date components are invalid.
    #[must_use]
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        tz_offset_seconds: i32,
    ) -> Option<Self> {
        let offset = FixedOffset::east_opt(tz_offset_seconds)?;
        offset
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .map(MessageDate)
    }

    /// Create a date in UTC from components.
    #[must_use]
    pub fn utc(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Option<Self> {
        Self::new(year, month, day, hour, minute, second, 0)
    }

    /// Create a date from a Unix timestamp (seconds since 1970-01-01 00:00:00 UTC).
    #[must_use]
    pub fn from_timestamp(secs: i64) -> Option<Self> {
        DateTime::from_timestamp(secs, 0).map(|dt| MessageDate(dt.fixed_offset()))
    }

    /// Create a date from a Unix timestamp with milliseconds.
    #[must_use]
    pub fn from_timestamp_millis(millis: i64) -> Option<Self> {
        DateTime::from_timestamp_millis(millis).map(|dt| MessageDate(dt.fixed_offset()))
    }

    /// Change the timezone offset without converting the time.
    ///
    /// This keeps the same year/month/day/hour/minute/second but labels them
    /// with a different timezone.
    ///
    /// # Example
    ///
    /// ```
    /// use simple_smtp::message::{MessageDate, TimeOffset};
    ///
    /// // You have 14:30 from an RTC, and you know you're in UTC+1
    /// let date = MessageDate::utc(2025, 12, 7, 14, 30, 0)
    ///     .unwrap()
    ///     .at_offset(TimeOffset::positive(1, 0))
    ///     .unwrap();
    ///
    /// assert!(date.to_string().contains("14:30:00 +0100"));
    /// ```
    #[must_use]
    pub fn at_offset(self, offset: TimeOffset) -> Option<Self> {
        let offset_seconds = offset.to_seconds();
        let offset = FixedOffset::east_opt(offset_seconds)?;
        let naive = self.0.naive_local();
        offset.from_local_datetime(&naive).single().map(MessageDate)
    }

    /// Get the current local time as a MessageDate.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn now() -> Self {
        MessageDate(chrono::Local::now().fixed_offset())
    }

    /// Get the current UTC time as a MessageDate.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn now_utc() -> Self {
        MessageDate(chrono::Utc::now().fixed_offset())
    }
}

impl fmt::Display for MessageDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use format() instead of to_rfc2822() to avoid allocation
        write!(f, "{}", self.0.format("%a, %d %b %Y %H:%M:%S %z"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Message builder - works without alloc!
// ═══════════════════════════════════════════════════════════════════════════

/// An RFC 5322 email message.
///
/// Required fields (Date, From, Message-ID) are set in the constructor.
/// Optional fields use builder methods like `with_to()`, `with_subject()`, etc.
///
/// For multiple recipients, format them yourself: `"a@x.com, b@y.com"`
#[derive(Debug, Clone)]
pub struct Message<'a> {
    date: MessageDate,
    from: &'a str,
    message_id: &'a str,
    to: Option<&'a str>,
    cc: Option<&'a str>,
    bcc: Option<&'a str>,
    subject: Option<&'a str>,
    reply_to: Option<&'a str>,
    in_reply_to: Option<&'a str>,
    references: Option<&'a str>,
    body: Option<&'a str>,
}

impl<'a> Message<'a> {
    /// Create a new message with the required RFC 5322 headers.
    ///
    /// - `date`: When the message was composed
    /// - `from`: The author(s), e.g. `"Me <me@example.com>"`
    /// - `message_id`: Unique identifier without angle brackets, e.g. `"abc123@example.com"`
    #[must_use]
    pub const fn new(date: MessageDate, from: &'a str, message_id: &'a str) -> Self {
        Self {
            date,
            from,
            message_id,
            to: None,
            cc: None,
            bcc: None,
            subject: None,
            reply_to: None,
            in_reply_to: None,
            references: None,
            body: None,
        }
    }

    // ── Required field getters ────────────────────────────────────────────────

    /// Get the Date header value.
    #[must_use]
    pub const fn date(&self) -> &MessageDate {
        &self.date
    }

    /// Get the From header value.
    #[must_use]
    pub const fn from(&self) -> &'a str {
        self.from
    }

    /// Get the Message-ID (without angle brackets).
    #[must_use]
    pub const fn message_id(&self) -> &'a str {
        self.message_id
    }

    // ── To ────────────────────────────────────────────────────────────────────

    /// Set the To header. For multiple recipients, format them yourself: `"a@x.com, b@y.com"`
    #[must_use]
    pub const fn with_to(mut self, to: &'a str) -> Self {
        self.to = Some(to);
        self
    }

    /// Get the To header value, if set.
    #[must_use]
    pub const fn to(&self) -> Option<&'a str> {
        self.to
    }

    // ── Cc ────────────────────────────────────────────────────────────────────

    /// Set the Cc header.
    #[must_use]
    pub const fn with_cc(mut self, cc: &'a str) -> Self {
        self.cc = Some(cc);
        self
    }

    /// Get the Cc header value, if set.
    #[must_use]
    pub const fn cc(&self) -> Option<&'a str> {
        self.cc
    }

    // ── Bcc ───────────────────────────────────────────────────────────────────

    /// Set the Bcc header.
    #[must_use]
    pub const fn with_bcc(mut self, bcc: &'a str) -> Self {
        self.bcc = Some(bcc);
        self
    }

    /// Get the Bcc header value, if set.
    #[must_use]
    pub const fn bcc(&self) -> Option<&'a str> {
        self.bcc
    }

    // ── Subject ───────────────────────────────────────────────────────────────

    /// Set the Subject header.
    #[must_use]
    pub const fn with_subject(mut self, s: &'a str) -> Self {
        self.subject = Some(s);
        self
    }

    /// Get the Subject header value, if set.
    #[must_use]
    pub const fn subject(&self) -> Option<&'a str> {
        self.subject
    }

    // ── Reply-To ──────────────────────────────────────────────────────────────

    /// Set the Reply-To header.
    #[must_use]
    pub const fn with_reply_to(mut self, r: &'a str) -> Self {
        self.reply_to = Some(r);
        self
    }

    /// Get the Reply-To header value, if set.
    #[must_use]
    pub const fn reply_to(&self) -> Option<&'a str> {
        self.reply_to
    }

    // ── In-Reply-To ───────────────────────────────────────────────────────────

    /// Set the In-Reply-To header.
    #[must_use]
    pub const fn with_in_reply_to(mut self, id: &'a str) -> Self {
        self.in_reply_to = Some(id);
        self
    }

    /// Get the In-Reply-To header value, if set.
    #[must_use]
    pub const fn in_reply_to(&self) -> Option<&'a str> {
        self.in_reply_to
    }

    // ── References ────────────────────────────────────────────────────────────

    /// Set the References header.
    #[must_use]
    pub const fn with_references(mut self, r: &'a str) -> Self {
        self.references = Some(r);
        self
    }

    /// Get the References header value, if set.
    #[must_use]
    pub const fn references(&self) -> Option<&'a str> {
        self.references
    }

    // ── Body ──────────────────────────────────────────────────────────────────

    /// Set the message body.
    #[must_use]
    pub const fn with_body(mut self, b: &'a str) -> Self {
        self.body = Some(b);
        self
    }

    /// Get the message body, if set.
    #[must_use]
    pub const fn body(&self) -> Option<&'a str> {
        self.body
    }

    // ── Formatting ────────────────────────────────────────────────────────────

    /// Generate a unique Message-ID using current time.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn generate_message_id(domain: &str) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        alloc::format!("{:x}@{}", ts, domain)
    }
}

impl fmt::Display for Message<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Required headers
        writeln!(f, "Date: {}\r", self.date)?;
        writeln!(f, "From: {}\r", self.from)?;
        writeln!(f, "Message-ID: <{}>\r", self.message_id)?;

        // Optional headers
        if let Some(to) = self.to {
            writeln!(f, "To: {}\r", to)?;
        }
        if let Some(cc) = self.cc {
            writeln!(f, "Cc: {}\r", cc)?;
        }
        if let Some(bcc) = self.bcc {
            writeln!(f, "Bcc: {}\r", bcc)?;
        }
        if let Some(reply_to) = self.reply_to {
            writeln!(f, "Reply-To: {}\r", reply_to)?;
        }
        if let Some(s) = self.subject {
            writeln!(f, "Subject: {}\r", s)?;
        }
        if let Some(r) = self.in_reply_to {
            writeln!(f, "In-Reply-To: {}\r", r)?;
        }
        if let Some(r) = self.references {
            writeln!(f, "References: {}\r", r)?;
        }

        // Blank line + body
        write!(f, "\r\n")?;
        if let Some(b) = self.body {
            write!(f, "{}", b)?;
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_format_rfc2822() {
        let d = MessageDate::utc(2025, 12, 7, 12, 30, 0)
            .unwrap()
            .at_offset(TimeOffset::positive(1, 0))
            .unwrap();
        assert!(d.to_string().contains("Dec 2025"));
        assert!(d.to_string().contains("12:30:00"));
        assert!(d.to_string().contains("+0100"));
    }

    #[test]
    fn date_invalid_returns_none() {
        assert!(MessageDate::utc(2025, 13, 1, 0, 0, 0).is_none());
        assert!(MessageDate::utc(2025, 2, 30, 0, 0, 0).is_none());
    }

    #[test]
    fn date_at_offset_changes_label_not_time() {
        let utc = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        let offset = utc.at_offset(TimeOffset::positive(5, 0)).unwrap();
        assert!(offset.to_string().contains("12:00:00 +0500"));
    }

    #[test]
<<<<<<< HEAD
=======
    fn date_at_offset_negative_hour_with_minutes() {
        // UTC-1:30 should be -0130
        let utc = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        let offset = utc.at_offset(TimeOffset::negative(1, 30)).unwrap();
        assert!(
            offset.to_string().contains("-0130"),
            "Expected -0130 but got: {}",
            offset
        );
    }

    #[test]
    fn date_at_offset_positive_hour_with_minutes() {
        // UTC+5:30 (like India) should be +0530
        let utc = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        let offset = utc.at_offset(TimeOffset::positive(5, 30)).unwrap();
        assert!(
            offset.to_string().contains("+0530"),
            "Expected +0530 but got: {}",
            offset
        );
    }

    #[test]
    fn date_at_offset_zero_hour_negative_minutes() {
        // Edge case: UTC-0:30 (rare but valid)
        let utc = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        let offset = utc.at_offset(TimeOffset::negative(0, 30)).unwrap();
        assert!(
            offset.to_string().contains("-0030"),
            "Expected -0030 but got: {}",
            offset
        );
    }

    #[test]
>>>>>>> ab1f08d (⭐ message: redesign at_offset API with TimeOffset enum)
    fn date_utc_convenience() {
        let d = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        assert!(d.to_string().contains("+0000"));
    }

    #[test]
    fn date_from_timestamp() {
        let d = MessageDate::from_timestamp(1735689600).unwrap();
        assert!(d.to_string().contains("Jan 2025"));
        assert!(d.to_string().contains("+0000"));
    }

    #[test]
    fn date_from_timestamp_millis() {
        let d = MessageDate::from_timestamp_millis(1_735_689_600_123).unwrap();
        assert!(d.to_string().contains("Jan 2025"));
    }

    #[test]
    fn message_getters() {
        let d = MessageDate::utc(2025, 12, 7, 12, 0, 0).unwrap();
        let msg = Message::new(d, "from@example.com", "id@example.com")
            .with_to("to@example.com")
            .with_subject("Subject line");

        // Required fields are always present
        assert_eq!(msg.from(), "from@example.com");
        assert_eq!(msg.message_id(), "id@example.com");

        // Optional fields return Option
        assert_eq!(msg.to(), Some("to@example.com"));
        assert_eq!(msg.subject(), Some("Subject line"));
        assert_eq!(msg.cc(), None);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn message_to_string() {
        let d = MessageDate::utc(2025, 12, 7, 14, 30, 0)
            .unwrap()
            .at_offset(TimeOffset::negative(5, 0))
            .unwrap();
        let msg = Message::new(d, "Sender <sender@example.com>", "abc123@example.com")
            .with_to("recipient@example.com")
            .with_subject("Test email")
            .with_body("Hello, world!");

        let s = msg.to_string();
        assert!(s.contains("Date:"));
        assert!(s.contains("From: Sender <sender@example.com>"));
        assert!(s.contains("To: recipient@example.com"));
        assert!(s.contains("Subject: Test email"));
        assert!(s.contains("Message-ID: <abc123@example.com>"));
        assert!(s.contains("\r\n\r\nHello, world!"));
    }
}
