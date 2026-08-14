//! Stream-9 error reports (S9F1/3/5/7/9/11).
//!
//! Source: `AbstractGem.S9fx` — primary DATA, stream=9, wbit=false,
//! body = `Secs2.Binary(refMsg.Header10Bytes())` (MHEAD).

use crate::hsms::{build_data_message, HsmsMessage};
use crate::secs2::{self, Secs2};
use crate::SecsMessage;

/// S9 function numbers (SEMI E5 Stream 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum S9Func {
    /// Unrecognized Device ID.
    UnrecognizedDeviceId = 1,
    /// Unrecognized Stream Type.
    UnrecognizedStream = 3,
    /// Unrecognized Function Type.
    UnrecognizedFunction = 5,
    /// Illegal Data.
    IllegalData = 7,
    /// Transaction Timer Timeout.
    TransactionTimeout = 9,
    /// Data Too Long.
    DataTooLong = 11,
}

impl S9Func {
    pub const fn code(self) -> i32 {
        self as i32
    }

    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::UnrecognizedDeviceId),
            3 => Some(Self::UnrecognizedStream),
            5 => Some(Self::UnrecognizedFunction),
            7 => Some(Self::IllegalData),
            9 => Some(Self::TransactionTimeout),
            11 => Some(Self::DataTooLong),
            _ => None,
        }
    }
}

/// SECS-II body for any S9Fx: Binary(MHEAD).
pub fn s9_body_from_header(header10: &[u8; 10]) -> secs2::Result<Secs2> {
    Secs2::binary(header10.to_vec())
}

/// SECS-II body from a reference message.
pub fn s9_body(ref_msg: &dyn SecsMessage) -> secs2::Result<Secs2> {
    s9_body_from_header(&ref_msg.header10_bytes())
}

/// Build S9Fx primary DATA message (no W-bit).
///
/// `AbstractGem.S9fx` → `Send(9, func, false, Binary(MHEAD))`.
pub fn build_s9_message(
    session_id: i32,
    func: S9Func,
    ref_msg: &dyn SecsMessage,
    sys: [u8; 4],
) -> Result<HsmsMessage, crate::hsms::Error> {
    build_s9_message_from_header(session_id, func, &ref_msg.header10_bytes(), sys)
}

/// Build S9Fx from raw MHEAD bytes.
pub fn build_s9_message_from_header(
    session_id: i32,
    func: S9Func,
    mhead: &[u8; 10],
    sys: [u8; 4],
) -> Result<HsmsMessage, crate::hsms::Error> {
    let body = s9_body_from_header(mhead).map_err(crate::hsms::Error::Secs2)?;
    build_data_message(session_id, 9, func.code(), false, body, sys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::HsmsMessage;
    use crate::SecsMessage as _;

    fn ref_data() -> HsmsMessage {
        // session=10, S1F1 W, sys=AA BB CC DD
        let h = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD,
        ];
        HsmsMessage::of_with_body(&h, Secs2::empty()).unwrap()
    }

    #[test]
    fn s9_func_codes() {
        assert_eq!(S9Func::UnrecognizedDeviceId.code(), 1);
        assert_eq!(S9Func::UnrecognizedStream.code(), 3);
        assert_eq!(S9Func::UnrecognizedFunction.code(), 5);
        assert_eq!(S9Func::IllegalData.code(), 7);
        assert_eq!(S9Func::TransactionTimeout.code(), 9);
        assert_eq!(S9Func::DataTooLong.code(), 11);
        assert_eq!(S9Func::from_code(5), Some(S9Func::UnrecognizedFunction));
        assert_eq!(S9Func::from_code(2), None);
    }

    #[test]
    fn s9_message_header_and_mhead_body() {
        let r = ref_data();
        let sys = [0x11, 0x22, 0x33, 0x44];
        let m = build_s9_message(10, S9Func::UnrecognizedFunction, &r, sys).unwrap();
        assert_eq!(m.get_stream(), 9);
        assert_eq!(m.get_function(), 5);
        assert!(!m.wbit());
        assert_eq!(m.session_id(), 10);
        let h = m.header10_bytes();
        assert_eq!(&h[6..10], &sys);
        // body Binary(MHEAD) = ref header
        let ref_h = r.header10_bytes();
        for i in 0..10 {
            assert_eq!(m.secs2().get_byte_at(&[i]).unwrap(), ref_h[i]);
        }
    }
}
