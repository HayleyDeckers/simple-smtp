//! Message formatting utilities.
//!
//! This module provides utilities for formatting email messages according to RFC 5322.

pub mod datetime;
pub use datetime::{DateTime, TimeZone};

pub mod address;
pub use address::EmailAddress;
