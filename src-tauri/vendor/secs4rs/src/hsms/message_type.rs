//! HSMS message type (p-Type / s-Type).
//!
//! Source: `Secs4Net.Hsms.HsmsMessageType`.

/// HSMS message type (rich enum → Rust enum + methods).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HsmsMessageType {
    Undefined,
    Data,
    SelectReq,
    SelectRsp,
    DeselectReq,
    DeselectRsp,
    LinktestReq,
    LinktestRsp,
    RejectReq,
    SeparateReq,
}

impl HsmsMessageType {
    /// Java/C# constant name (`ToString()` parity).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Undefined => "UNDEFINED",
            Self::Data => "DATA",
            Self::SelectReq => "SELECT_REQ",
            Self::SelectRsp => "SELECT_RSP",
            Self::DeselectReq => "DESELECT_REQ",
            Self::DeselectRsp => "DESELECT_RSP",
            Self::LinktestReq => "LINKTEST_REQ",
            Self::LinktestRsp => "LINKTEST_RSP",
            Self::RejectReq => "REJECT_REQ",
            Self::SeparateReq => "SEPARATE_REQ",
        }
    }

    /// p-Type wire byte.
    pub const fn p_type(self) -> u8 {
        match self {
            Self::Undefined => 0x80,
            _ => 0,
        }
    }

    /// s-Type wire byte.
    pub const fn s_type(self) -> u8 {
        match self {
            Self::Undefined => 0x80,
            Self::Data => 0,
            Self::SelectReq => 1,
            Self::SelectRsp => 2,
            Self::DeselectReq => 3,
            Self::DeselectRsp => 4,
            Self::LinktestReq => 5,
            Self::LinktestRsp => 6,
            Self::RejectReq => 7,
            Self::SeparateReq => 9,
        }
    }

    /// Lookup by p/s codes (excludes UNDEFINED from match; falls back to Undefined).
    pub fn get(p: u8, s: u8) -> Self {
        for t in Self::defined() {
            if t.p_type() == p && t.s_type() == s {
                return *t;
            }
        }
        Self::Undefined
    }

    /// True if any defined type uses this p-Type.
    pub fn support_p_type(p: u8) -> bool {
        Self::defined().iter().any(|t| t.p_type() == p)
    }

    /// True if any defined type uses this s-Type.
    pub fn support_s_type(s: u8) -> bool {
        Self::defined().iter().any(|t| t.s_type() == s)
    }

    /// Defined types only (no UNDEFINED).
    pub const fn defined() -> &'static [Self] {
        &[
            Self::Data,
            Self::SelectReq,
            Self::SelectRsp,
            Self::DeselectReq,
            Self::DeselectRsp,
            Self::LinktestReq,
            Self::LinktestRsp,
            Self::RejectReq,
            Self::SeparateReq,
        ]
    }
}

impl std::fmt::Display for HsmsMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsms_message_type() {
        // Secs4Net.Tests: hsms-message-type
        assert_eq!(HsmsMessageType::SelectReq.p_type(), 0);
        assert_eq!(HsmsMessageType::SelectReq.s_type(), 1);
        assert_eq!(HsmsMessageType::get(0, 1), HsmsMessageType::SelectReq);
        assert_eq!(HsmsMessageType::get(0, 9), HsmsMessageType::SeparateReq);
        assert_eq!(HsmsMessageType::SelectReq.to_string(), "SELECT_REQ");
    }

    #[test]
    fn hsms_ptype_stype_support() {
        // Secs4Net.Tests: hsms-pType-sType-support
        assert!(HsmsMessageType::support_s_type(1)); // SELECT_REQ s-type
        assert!(!HsmsMessageType::support_s_type(99));
        // p-Type: DATA/control use p=0
        assert!(HsmsMessageType::support_p_type(0));
        assert!(!HsmsMessageType::support_p_type(99));
    }
}
