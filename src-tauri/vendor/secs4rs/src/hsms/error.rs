//! HSMS error kinds (distinguishable failure reasons).

use std::fmt;

/// HSMS / message construction errors.
// PartialEq only: `TimeoutT3.primary` embeds `HsmsMessage` (Secs2 may hold floats → no Eq).
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Header is not exactly 10 bytes.
    HeaderByteLength,
    /// Message length field lower than 10 (body + header).
    LengthBytesLowerThanTen,
    /// Control message body longer than allowed.
    ControlMessageLengthGreaterThanTen,
    /// Incomplete buffer / short read.
    Truncated,
    /// SECS-II body parse failed.
    Secs2(crate::secs2::Error),
    /// Socket peer closed / zero-length read (detect terminate).
    DetectTerminate,
    /// T3 data-message reply timeout (W-bit).
    ///
    /// Carries the unanswered primary (`SecsWaitReplyMessageException` reference message).
    TimeoutT3 {
        /// Primary DATA that did not receive a reply.
        primary: super::message::HsmsMessage,
    },
    /// T6 control-transaction reply timeout (SELECT/LINKTEST/…).
    TimeoutT6,
    /// T7 not-selected (passive wait for SELECT.req) timeout.
    TimeoutT7,
    /// T8 network inter-character / incomplete-frame read timeout.
    TimeoutT8,
    /// Passive first message was not SELECT.req.
    PassiveNotSelectRequest,
    /// Peer replied with REJECT.req to a transaction.
    Reject,
    /// Channel already shut down.
    ChannelShutdown,
    /// I/O error (timeout, broken pipe, etc.).
    Io(String),
    /// Other HSMS protocol error.
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderByteLength => write!(f, "HSMS header must be exactly 10 bytes"),
            Self::LengthBytesLowerThanTen => write!(f, "HSMS length bytes lower than ten"),
            Self::ControlMessageLengthGreaterThanTen => {
                write!(f, "HSMS control message length greater than ten")
            }
            Self::Truncated => write!(f, "HSMS wire truncated"),
            Self::Secs2(e) => write!(f, "SECS-II: {e}"),
            Self::DetectTerminate => write!(f, "HSMS detect terminate"),
            Self::TimeoutT3 { .. } => write!(f, "HSMS T3 timeout"),
            Self::TimeoutT6 => write!(f, "HSMS T6 timeout"),
            Self::TimeoutT7 => write!(f, "HSMS T7 timeout"),
            Self::TimeoutT8 => write!(f, "HSMS T8 timeout"),
            Self::PassiveNotSelectRequest => {
                write!(f, "HSMS-SS passive first message not SELECT.req")
            }
            Self::Reject => write!(f, "HSMS reject"),
            Self::ChannelShutdown => write!(f, "HSMS channel already shutdown"),
            Self::Io(m) => write!(f, "HSMS I/O: {m}"),
            Self::Protocol(m) => write!(f, "HSMS: {m}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::DetectTerminate
        } else if e.kind() == std::io::ErrorKind::TimedOut
            || e.kind() == std::io::ErrorKind::WouldBlock
        {
            // Live HSMS reader maps this to TimeoutT8; other call sites keep Io.
            Self::Io(format!("timeout: {e}"))
        } else {
            Self::Io(e.to_string())
        }
    }
}

/// Map socket read timeout to T8 (frame assembly on live channel).
pub fn io_timeout_as_t8(e: Error) -> Error {
    match e {
        Error::Io(ref s) if s.starts_with("timeout:") => Error::TimeoutT8,
        other => other,
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
