//! SECS-II error kinds (distinguishable failure reasons from Secs4Net).

use std::fmt;

/// SECS-II error (covers Secs2*Exception family at the call-site level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Wrong item type for the requested accessor.
    IllegalDataFormat(&'static str),
    /// Index path / element out of range.
    IndexOutOfBounds,
    /// Body / element count exceeds 3-byte length field.
    LengthByteOutOfRange,
    /// Parse ran out of bytes or trailing garbage.
    BytesParse(&'static str),
    /// Format code not recognized.
    UnsupportedDataFormat,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IllegalDataFormat(m) => write!(f, "illegal data format: {m}"),
            Self::IndexOutOfBounds => write!(f, "index out of bounds"),
            Self::LengthByteOutOfRange => write!(f, "length byte out of range"),
            Self::BytesParse(m) => write!(f, "bytes parse: {m}"),
            Self::UnsupportedDataFormat => write!(f, "unsupported data format"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
