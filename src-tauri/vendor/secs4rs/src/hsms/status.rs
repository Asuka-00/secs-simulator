//! HSMS select / deselect / reject status codes.
//!
//! Source: `HsmsMessageSelectStatus`, `HsmsMessageDeselectStatus`, `HsmsMessageRejectReason`.

use super::message::HsmsMessage;
use super::message_type::HsmsMessageType;

/// SELECT.rsp status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectStatus {
    NotSelectRsp,
    Unknown,
    Success,
    Actived,
    NotReady,
    AlreadyUsed,
    EntityUnknown,
    EntityAlreadyUsed,
    EntityActived,
}

impl SelectStatus {
    pub const fn status_code(self) -> u8 {
        match self {
            Self::NotSelectRsp | Self::Unknown => 0xFF,
            Self::Success => 0,
            Self::Actived => 1,
            Self::NotReady => 2,
            Self::AlreadyUsed => 3,
            Self::EntityUnknown => 4,
            Self::EntityAlreadyUsed => 5,
            Self::EntityActived => 6,
        }
    }

    /// Lookup by status byte (skips NotSelectRsp/Unknown sentinels).
    pub fn from_code(b: u8) -> Self {
        for s in [
            Self::Success,
            Self::Actived,
            Self::NotReady,
            Self::AlreadyUsed,
            Self::EntityUnknown,
            Self::EntityAlreadyUsed,
            Self::EntityActived,
        ] {
            if s.status_code() == b {
                return s;
            }
        }
        Self::Unknown
    }

    /// From message: SELECT_RSP → byte[3]; else NotSelectRsp.
    pub fn from_message(msg: &HsmsMessage) -> Self {
        if msg.message_type() == HsmsMessageType::SelectRsp {
            Self::from_code(msg.header10_bytes()[3])
        } else {
            Self::NotSelectRsp
        }
    }
}

/// DESELECT.rsp status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeselectStatus {
    NotDeselectRsp,
    Unknown,
    Success,
    NoSelected,
    Failed,
}

impl DeselectStatus {
    pub const fn status_code(self) -> u8 {
        match self {
            Self::NotDeselectRsp | Self::Unknown => 0xFF,
            Self::Success => 0,
            Self::NoSelected => 1,
            Self::Failed => 2,
        }
    }

    pub fn from_code(b: u8) -> Self {
        for s in [Self::Success, Self::NoSelected, Self::Failed] {
            if s.status_code() == b {
                return s;
            }
        }
        Self::Unknown
    }

    pub fn from_message(msg: &HsmsMessage) -> Self {
        if msg.message_type() == HsmsMessageType::DeselectRsp {
            Self::from_code(msg.header10_bytes()[3])
        } else {
            Self::NotDeselectRsp
        }
    }
}

/// REJECT.req reason byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectReason {
    NotRejectReq,
    Unknown,
    NotSupportTypeS,
    NotSupportTypeP,
    TransactionNotOpen,
    NotSelected,
}

impl RejectReason {
    pub const fn reason_code(self) -> u8 {
        match self {
            Self::NotRejectReq | Self::Unknown => 0xFF,
            Self::NotSupportTypeS => 1,
            Self::NotSupportTypeP => 2,
            Self::TransactionNotOpen => 3,
            Self::NotSelected => 4,
        }
    }

    /// Lookup by byte; skips Unknown (0xFF) so NotRejectReq matches 0xFF first if listed…
    /// Parity with C#: skip only UNKNOWN; NOT_REJECT_REQ also 0xFF is checked first in values array
    /// order: NOT_REJECT_REQ, UNKNOWN, … — after skip UNKNOWN, NOT_REJECT_REQ matches 0xFF.
    pub fn from_code(b: u8) -> Self {
        for r in [
            Self::NotRejectReq,
            // Unknown skipped in C# loop
            Self::NotSupportTypeS,
            Self::NotSupportTypeP,
            Self::TransactionNotOpen,
            Self::NotSelected,
        ] {
            if r.reason_code() == b {
                return r;
            }
        }
        Self::Unknown
    }

    pub fn from_message(msg: &HsmsMessage) -> Self {
        if msg.message_type() == HsmsMessageType::RejectReq {
            Self::from_code(msg.header10_bytes()[3])
        } else {
            Self::NotRejectReq
        }
    }
}
