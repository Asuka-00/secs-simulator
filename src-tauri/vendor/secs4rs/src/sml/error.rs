//! SML parse errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Parse(&'static str),
    StreamOutOfRange,
    FunctionOutOfRange,
    NotFoundEndPeriod,
    DataItem(&'static str),
    Secs2(crate::secs2::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "SML parse: {m}"),
            Self::StreamOutOfRange => write!(f, "stream out of range"),
            Self::FunctionOutOfRange => write!(f, "function out of range"),
            Self::NotFoundEndPeriod => write!(f, "not found end period"),
            Self::DataItem(m) => write!(f, "SML data item: {m}"),
            Self::Secs2(e) => write!(f, "SECS-II: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
