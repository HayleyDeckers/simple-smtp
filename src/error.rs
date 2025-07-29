use crate::smtp::Extensions;

//todo: no thiserror so as not to pull in syn and keep embedded build times fast
/// errors that originated from the SMTP protocol
/// Does not track io errors or expected errors (like failed authentication)
#[derive(Debug)]
pub enum MalformedError {
    // we error out on lines which aren't terminated with \r\n
    // because the RFC says that a client should not send bare CR or LF
    // https://datatracker.ietf.org/doc/html/rfc5321#section-2.3.8
    InvalidLineTermination,
    InvalidEncoding,
    NoCode,
    UnexpectedCode {
        expected: &'static [u16],
        actual: u16,
    },
    CodeChanged {
        old_code: u16,
        new_code: u16,
    },
    UnexpectedEof,
}

impl core::error::Error for MalformedError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::fmt::Display for MalformedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MalformedError::InvalidLineTermination => write!(f, "Invalid line termination"),
            MalformedError::InvalidEncoding => write!(f, "Invalid encoding"),
            MalformedError::NoCode => write!(f, "No code"),
            MalformedError::UnexpectedCode { expected, actual } => {
                write!(
                    f,
                    "Recieved unexpected code {}, expected one of {:?}",
                    actual, expected
                )
            }
            MalformedError::CodeChanged { old_code, new_code } => {
                write!(
                    f,
                    "code changed midway through a response. Was {}, now {}",
                    old_code, new_code
                )
            }
            MalformedError::UnexpectedEof => write!(f, "Unexpected EOF reached"),
        }
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    AuthorizationError,
    LineTooLong,
    #[cfg(feature = "lettre")]
    NoSender,
    UnsupportedExtension(Extensions<'static>),
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProtocolError::AuthorizationError => write!(f, "Invalid Auth"),
            ProtocolError::LineTooLong => write!(f, "Line too long"),
            #[cfg(feature = "lettre")]
            ProtocolError::NoSender => write!(f, "Missing \"from\" address on lettre envelope"),
            ProtocolError::UnsupportedExtension(ext) => {
                write!(f, "Extension {ext} not supported")
            }
        }
    }
}

impl core::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

/// any error that can occur in the SMTP transport layer
/// can be categorized into three categories:
/// - IO errors
/// - Protocol errors
/// - General SMTP related errors that are expected within the protocol
///   like "message too long" or "not authenticated"
#[derive(Debug)]
pub enum Error<T: core::error::Error> {
    IoError(T),
    ProtocolError(ProtocolError),
    MalformedError(MalformedError),
}

impl<T: core::error::Error> core::fmt::Display for Error<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::IoError(e) => write!(f, "IO Error: {e}"),
            Error::ProtocolError(e) => e.fmt(f),
            Error::MalformedError(e) => e.fmt(f),
        }
    }
}

impl<T: core::error::Error + 'static> core::error::Error for Error<T> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Error::IoError(e) => Some(e),
            Error::ProtocolError(e) => e.source(),
            Error::MalformedError(e) => e.source(),
        }
    }
}

impl<T: core::error::Error> From<ProtocolError> for Error<T> {
    fn from(e: ProtocolError) -> Self {
        Error::ProtocolError(e)
    }
}

impl<T: core::error::Error> From<MalformedError> for Error<T> {
    fn from(e: MalformedError) -> Self {
        Error::MalformedError(e)
    }
}
