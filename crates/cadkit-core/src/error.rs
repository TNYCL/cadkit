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
    /// Input or output would exceed a configured [`crate::Limits`] value or a capacity of
    /// the file format.
    #[error("limit exceeded: {message}")]
    LimitExceeded {
        /// Which limit was hit.
        limit: LimitKind,
        /// What was being counted when it was hit.
        message: String,
    },
    /// Document JSON written for a [`crate::SchemaVersion`] this crate cannot read.
    #[error("unsupported document schema version {found}; this cadkit reads version {supported}")]
    SchemaVersion {
        /// Version found in the input.
        found: u64,
        /// Version this crate reads.
        supported: u32,
    },
    /// Input is not a recognized drawing format.
    #[error("unrecognized file format")]
    UnknownFormat,
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The limit behind an [`Error::LimitExceeded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    /// [`crate::Limits::max_input_bytes`].
    InputBytes,
    /// [`crate::Limits::max_decompressed_bytes`], the budget for decoded and generated data.
    DecompressedBytes,
    /// [`crate::Limits::max_objects`].
    Objects,
    /// [`crate::Limits::max_depth`].
    Depth,
    /// [`crate::Limits::max_string_bytes`].
    StringBytes,
    /// [`crate::Limits::max_vertices`].
    Vertices,
    /// [`crate::Limits::max_total_vertices`].
    TotalVertices,
    /// A computation work budget, such as `WalkOptions::max_work` or a validation budget.
    Work,
    /// The size cap of an output being written.
    Output,
    /// A value does not fit a field of the file format or the host's address range;
    /// no configured limit is involved.
    Capacity,
}

impl Error {
    /// Shorthand for [`Error::LimitExceeded`].
    pub fn limit(limit: LimitKind, message: String) -> Self {
        Self::LimitExceeded { limit, message }
    }

    /// Shorthand for [`Error::Invalid`].
    pub fn invalid(offset: u64, message: impl Into<String>) -> Self {
        Self::Invalid {
            offset,
            message: message.into(),
        }
    }
}
