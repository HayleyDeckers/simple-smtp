//! RFC 5322 Internet Message Format implementation.
//!
//! Provides the Message struct for constructing RFC 5322 compliant emails.
//!
//! This module re-exports `MessageDate` from the message module (added in the timestamp MR)
//! and adds the `Message` struct for constructing RFC 5322 compliant emails.

use core::fmt;

// Re-export MessageDate from the message module (defined in timestamp MR)
pub use crate::message::MessageDate;

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

#[cfg(test)]
mod tests {
    use super::*;

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
            .at_offset(-5, 0)
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
