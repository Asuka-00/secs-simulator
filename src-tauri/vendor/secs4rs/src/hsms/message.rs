//! HSMS message from 10-byte header + optional SECS-II body.
//!
//! Source: `HsmsMessage` / `AbstractHsmsMessage`.

use crate::secs2::Secs2;
use crate::secs_message::SecsMessage;

use super::error::{Error, Result};
use super::message_type::HsmsMessageType;

const HEADER_SIZE: usize = 10;

/// HSMS message (header + SECS-II body).
#[derive(Debug, Clone, PartialEq)]
pub struct HsmsMessage {
    header: [u8; HEADER_SIZE],
    body: Secs2,
    msg_type: HsmsMessageType,
    is_data: bool,
    session_id: i32,
    stream: i32,
    function: i32,
    wbit: bool,
}

impl HsmsMessage {
    /// Head-only message (`HsmsMessage.Of(header)` with empty body).
    pub fn of(header: &[u8]) -> Result<Self> {
        Self::of_with_body(header, Secs2::empty())
    }

    /// Header + body (`HsmsMessage.Of(header, body)`).
    pub fn of_with_body(header: &[u8], body: Secs2) -> Result<Self> {
        if header.len() != HEADER_SIZE {
            return Err(Error::HeaderByteLength);
        }
        let mut h = [0u8; HEADER_SIZE];
        h.copy_from_slice(header);

        let msg_type = HsmsMessageType::get(h[4], h[5]);
        let is_data = msg_type == HsmsMessageType::Data;

        // Session id = header[0..1] BE; control 0xFFFF → -1 (AbstractHsmsMessage).
        let session_u = ((u16::from(h[0]) << 8) | u16::from(h[1])) as i32;
        let session_id = if is_data {
            session_u
        } else if session_u == 0x0000_FFFF {
            -1
        } else {
            session_u
        };

        let (stream, function, wbit) = if is_data {
            let strm = (h[2] as i32) & 0x7F;
            let func = (h[3] as i32) & 0xFF;
            let w = (h[2] as i32) & 0x80 == 0x80;
            (strm, func, w)
        } else {
            (-1, -1, false)
        };

        Ok(Self {
            header: h,
            body,
            msg_type,
            is_data,
            session_id,
            stream,
            function,
            wbit,
        })
    }

    pub fn message_type(&self) -> HsmsMessageType {
        self.msg_type
    }

    pub fn is_data_message(&self) -> bool {
        self.is_data
    }

    pub fn p_type(&self) -> u8 {
        self.msg_type.p_type()
    }

    pub fn s_type(&self) -> u8 {
        self.msg_type.s_type()
    }

    pub fn header10_bytes(&self) -> [u8; HEADER_SIZE] {
        self.header
    }

    pub fn secs2(&self) -> &Secs2 {
        &self.body
    }

    pub fn session_id(&self) -> i32 {
        self.session_id
    }

    pub fn device_id(&self) -> i32 {
        self.session_id
    }

    pub fn get_stream(&self) -> i32 {
        self.stream
    }

    pub fn get_function(&self) -> i32 {
        self.function
    }

    pub fn wbit(&self) -> bool {
        self.wbit
    }

    /// System bytes key: header bytes 6..9 as big-endian u32 (as i64 for parity).
    pub fn system_bytes_key(&self) -> i64 {
        let b = &self.header;
        (((b[6] as i64) << 24) & 0xFF00_0000)
            | (((b[7] as i64) << 16) & 0x00FF_0000)
            | (((b[8] as i64) << 8) & 0x0000_FF00)
            | ((b[9] as i64) & 0x0000_00FF)
    }
}

impl SecsMessage for HsmsMessage {
    fn get_stream(&self) -> i32 {
        self.stream
    }

    fn get_function(&self) -> i32 {
        self.function
    }

    fn wbit(&self) -> bool {
        self.wbit
    }

    fn secs2(&self) -> &Secs2 {
        &self.body
    }

    fn device_id(&self) -> i32 {
        self.session_id
    }

    fn session_id(&self) -> i32 {
        self.session_id
    }

    fn header10_bytes(&self) -> [u8; 10] {
        self.header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsms_message_of_header() {
        // SELECT_REQ header: p=0 s=1 at bytes[4],[5]
        let header = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01];
        let m = HsmsMessage::of(&header).unwrap();
        assert_eq!(m.message_type(), HsmsMessageType::SelectReq);
        assert!(!m.is_data_message());
        assert_eq!(m.session_id(), -1); // 0xFFFF on control
    }

    #[test]
    fn hsms_header_length_validation() {
        let r = HsmsMessage::of(&[0x00, 0x01]);
        assert_eq!(r, Err(Error::HeaderByteLength));
    }

    #[test]
    fn hsms_data_message() {
        // DATA: p=0 s=0; W-bit on stream high bit. S1F1 W.
        let header = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
        ];
        let m = HsmsMessage::of(&header).unwrap();
        assert!(m.is_data_message());
        assert_eq!(m.message_type(), HsmsMessageType::Data);
        assert_eq!(m.get_stream(), 1);
        assert_eq!(m.get_function(), 1);
        assert!(m.wbit());
        assert_eq!(m.session_id(), 10);
    }

    #[test]
    fn hsms_message_implements_secs_message() {
        use crate::secs_message::SecsMessage as SM;
        let header = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
        ];
        let m = HsmsMessage::of(&header).unwrap();
        assert_eq!(SM::get_stream(&m), 1);
        assert_eq!(SM::get_function(&m), 1);
        assert!(SM::wbit(&m));
        assert_eq!(SM::session_id(&m), 10);
        assert_eq!(SM::device_id(&m), 10);
        assert_eq!(SM::header10_bytes(&m)[0], 0x00);
    }

    #[test]
    fn hsms_message_header_preserved() {
        // Secs4Net.Tests: hsms-message-header-preserved
        // SELECT_REQ: device=0x0005, p/s=0/1, sysbytes=AA BB CC DD
        let primary_header = [
            0x00, 0x05, 0x00, 0x00, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD,
        ];
        let m = HsmsMessage::of(&primary_header).unwrap();
        let h = m.header10_bytes();
        assert_eq!(h.len(), 10);
        assert_eq!(h[6], 0xAA);
        assert_eq!(h[1], 0x05);
        assert_eq!(h[7], 0xBB);
        assert_eq!(h[8], 0xCC);
        assert_eq!(h[9], 0xDD);
    }
}
