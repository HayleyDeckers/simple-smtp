use crate::smtp::Extensions;

//todo: no thiserror so as not to pull in syn and keep embedded build times fast
/// errors that originated from the SMTP protocol
/// Does not track io errors or expected errors (like failed authentication)
#[derive(Debug, thiserror::Error)]
pub enum MalformedError {
    // we error out on lines which aren't terminated with \r\n
    // because the RFC says that a client should not send bare CR or LF
    // https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.8
    #[error("Invalid line termination")]
    InvalidLineTermination,
    #[error("Non utf-8 payload")]
    InvalidEncoding,
    #[error("No Code")]
    NoCode,
    #[error("Recieved unexpected code {actual}, expected one of {expected:?}")]
    UnexpectedCode {
        expected: &'static [u16],
        actual: u16,
    },
    #[error("code changed midway through a response. Was {old_code}, now {new_code}")]
    CodeChanged { old_code: u16, new_code: u16 },
    #[error("Unexpected EOF reached")]
    UnexpectedEof,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Invalid Auth")]
    AuthorizationError,
    #[error("Line too long")]
    LineTooLong,
    #[cfg(feature = "lettre")]
    #[error("Missing \"from\" address on lettre envelope")]
    NoSender,
    // only works with static because Error trait requires it,
    // but shouldnt be an issue because if we're checking for support we're most likely using a non-str variant
    #[error("Extension {} not supported", .0)]
    UnsupportedExtension(Extensions<'static>),
}

/// any error that can occur in the SMTP transport layer
/// can be categorized into three categories:
/// - IO errors
/// - Protocol errors
/// - General SMTP related errors that are expected within the protocol
///   like "message too long" or "not authenticated"
#[derive(Debug, thiserror::Error)]
pub enum Error<T: core::error::Error> {
    #[error("IO Error: {0}")]
    IoError(T),
    #[error(transparent)]
    ProtocolError(#[from] ProtocolError),
    #[error(transparent)]
    MalformedError(#[from] MalformedError),
}
