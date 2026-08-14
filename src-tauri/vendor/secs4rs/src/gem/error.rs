//! GEM SxFy send/reply path errors.

use crate::hsms::Error as HsmsError;
use crate::hsms::HsmsMessage;
use crate::secs2;
use crate::SecsMessage;

/// GEM message path errors (parse / unexpected reply / HSMS).
#[derive(Debug, Clone, PartialEq)]
pub enum GemError {
    Hsms(HsmsError),
    Secs2(secs2::Error),
    /// Reply missing or not the expected SxFy.
    UnexpectedReply {
        expected_stream: i32,
        expected_function: i32,
        got_stream: Option<i32>,
        got_function: Option<i32>,
    },
}

impl std::fmt::Display for GemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hsms(e) => write!(f, "HSMS: {e}"),
            Self::Secs2(e) => write!(f, "SECS-II: {e}"),
            Self::UnexpectedReply {
                expected_stream,
                expected_function,
                got_stream,
                got_function,
            } => write!(
                f,
                "unexpected reply: expected S{expected_stream}F{expected_function}, got S{:?}F{:?}",
                got_stream, got_function
            ),
        }
    }
}

impl std::error::Error for GemError {}

impl From<HsmsError> for GemError {
    fn from(e: HsmsError) -> Self {
        Self::Hsms(e)
    }
}

impl From<secs2::Error> for GemError {
    fn from(e: secs2::Error) -> Self {
        Self::Secs2(e)
    }
}

/// Require reply with exact stream/function.
pub(crate) fn expect_reply(
    reply: Option<HsmsMessage>,
    stream: i32,
    function: i32,
) -> Result<HsmsMessage, GemError> {
    match reply {
        Some(m) if m.get_stream() == stream && m.get_function() == function => Ok(m),
        Some(m) => Err(GemError::UnexpectedReply {
            expected_stream: stream,
            expected_function: function,
            got_stream: Some(m.get_stream()),
            got_function: Some(m.get_function()),
        }),
        None => Err(GemError::UnexpectedReply {
            expected_stream: stream,
            expected_function: function,
            got_stream: None,
            got_function: None,
        }),
    }
}
