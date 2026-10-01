//! Error type shared by all cadkit crates.

/// Result alias using [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why reading or writing failed.
///
/// Readers fail only when nothing sensible can be produced; recoverable issues
/// become [`crate::Warning`]s on the document instead.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Input ended before a structure was complete.
    #[error("unexpected end of data at offset {offset}: needed {needed} more bytes")]
    Truncated {
        /// Offset where the read started.
        offset: u64,
        /// Bytes that were missing.
        needed: u64,
    },
    /// Input violates the format.
    #[error("invalid data at offset {offset}: {message}")]
    Invalid {
        /// Offset of the offending data.
        offset: u64,
        /// What is wrong.
        message: String,
    },
    /// Valid input that uses a feature cadkit does not implement.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Input would exceed a configured [`crate::Limits`] value.
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    /// Input is not a recognized drawing format.
    #[error("unrecognized file format")]
    UnknownFormat,
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Shorthand for [`Error::Invalid`].
    pub fn invalid(offset: u64, message: impl Into<String>) -> Self {
        Self::Invalid {
            offset,
            message: message.into(),
        }
    }
}
