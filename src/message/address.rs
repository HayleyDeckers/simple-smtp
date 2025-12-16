//! RFC 5321/5322 email address parsing.
//!
//! Provides typed email address parsing with validation.

#[cfg(feature = "alloc")]
use alloc::string::String;
use core::fmt;

/// An RFC 5321/5322 compliant email address.
///
/// This type validates email addresses according to the Mailbox syntax:
/// `Local-part "@" ( Domain / address-literal )`
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailAddress {
    local_part: String,
    domain: String,
}

#[cfg(feature = "alloc")]
impl EmailAddress {
    /// Get the local part of the email address.
    #[must_use]
    pub fn local_part(&self) -> &str {
        &self.local_part
    }

    /// Get the domain part of the email address.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }
}

#[cfg(feature = "alloc")]
impl core::str::FromStr for EmailAddress {
    type Err = ParseError;

    fn from_str(_: &str) -> Result<Self, Self::Err> {
        // TODO: Implement RFC 5321/5322 parsing
        // - Support dot-string and quoted-string local parts
        // - Support address literals like user@[192.0.2.1]
        // - Proper error messages
        Err(ParseError::NotImplemented)
    }
}

#[cfg(feature = "alloc")]
impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.local_part, self.domain)
    }
}

/// Errors that can occur when parsing an email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Parsing not yet implemented.
    NotImplemented,
    // TODO: Add more specific error variants
    // InvalidLocalPart,
    // InvalidDomain,
    // InvalidAddressLiteral,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseError::NotImplemented => write!(f, "Email address parsing not yet implemented"),
        }
    }
}

impl core::error::Error for ParseError {}

#[cfg(test)]
#[cfg(feature = "alloc")]
mod tests {
    use super::*;

    #[test]
    fn email_address_display() {
        let addr = EmailAddress {
            local_part: "user".to_string(),
            domain: "example.com".to_string(),
        };
        assert_eq!(addr.to_string(), "user@example.com");
    }
}
