//! HSMS TCP wire framing: 4-byte BE length + 10-byte header + SECS-II body.
//!
//! Source: `AbstractHsmsAsynchronousSocketChannelFacade` send/receive loops.
//! Length field = header(10) + body bytes (not including the 4 length octets).

use crate::secs2::Secs2;

use super::error::{Error, Result};
use super::message::HsmsMessage;

/// Encode message to HSMS wire bytes (one complete frame).
pub fn encode_frame(msg: &HsmsMessage) -> Result<Vec<u8>> {
    let header = msg.header10_bytes();
    let body_wire = body_wire_bytes(msg);
    let len = 10u32
        .checked_add(body_wire.len() as u32)
        .ok_or(Error::Protocol("length overflow"))?;
    if len < 10 {
        return Err(Error::LengthBytesLowerThanTen);
    }
    // Control messages must be header-only (length == 10).
    if !msg.is_data_message() && len != 10 {
        return Err(Error::ControlMessageLengthGreaterThanTen);
    }

    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&body_wire);
    Ok(out)
}

/// Decode one complete frame from `buf` (must contain full message).
/// Returns `(message, bytes_consumed)`.
pub fn decode_frame(buf: &[u8]) -> Result<(HsmsMessage, usize)> {
    if buf.len() < 4 {
        return Err(Error::Truncated);
    }
    let msg_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if msg_len < 10 {
        return Err(Error::LengthBytesLowerThanTen);
    }
    let total = 4 + msg_len;
    if buf.len() < total {
        return Err(Error::Truncated);
    }
    let header = &buf[4..14];
    let body_bytes = &buf[14..total];

    let body = if body_bytes.is_empty() {
        Secs2::empty()
    } else {
        Secs2::parse_bytes(body_bytes).map_err(Error::Secs2)?
    };

    let msg = HsmsMessage::of_with_body(header, body)?;
    if !msg.is_data_message() && msg_len != 10 {
        return Err(Error::ControlMessageLengthGreaterThanTen);
    }
    Ok((msg, total))
}

/// Decode using pre-split length / header / body (socket receive path shape).
pub fn build_from_parts(header10: &[u8], body_chunks: &[Vec<u8>]) -> Result<HsmsMessage> {
    let body = if body_chunks.is_empty() || body_chunks.iter().all(|c| c.is_empty()) {
        Secs2::empty()
    } else {
        Secs2::parse(body_chunks).map_err(Error::Secs2)?
    };
    let msg = HsmsMessage::of_with_body(header10, body)?;
    let body_len: usize = body_chunks.iter().map(|c| c.len()).sum();
    let msg_len = 10 + body_len;
    if !msg.is_data_message() && msg_len != 10 {
        return Err(Error::ControlMessageLengthGreaterThanTen);
    }
    Ok(msg)
}

fn body_wire_bytes(msg: &HsmsMessage) -> Vec<u8> {
    if matches!(msg.secs2(), Secs2::Empty) {
        return Vec::new();
    }
    // Flatten SECS-II get_bytes_list (same as C# send path with chunked body).
    msg.secs2()
        .get_bytes_list(1024)
        .into_iter()
        .flatten()
        .collect()
}

impl HsmsMessage {
    /// Encode this message as a single HSMS TCP frame.
    pub fn to_wire(&self) -> Result<Vec<u8>> {
        encode_frame(self)
    }

    /// Parse one HSMS frame from a complete buffer.
    pub fn from_wire(buf: &[u8]) -> Result<Self> {
        decode_frame(buf).map(|(m, _)| m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::builder::build_select_response;
    use crate::hsms::message_type::HsmsMessageType;
    use crate::hsms::status::SelectStatus;
    use crate::secs2::Secs2;

    #[test]
    fn control_select_req_wire() {
        let header = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01];
        let m = HsmsMessage::of(&header).unwrap();
        let w = m.to_wire().unwrap();
        // length=10 BE + 10 header
        assert_eq!(w.len(), 14);
        assert_eq!(&w[0..4], &[0, 0, 0, 10]);
        assert_eq!(&w[4..14], &header);

        let back = HsmsMessage::from_wire(&w).unwrap();
        assert_eq!(back.message_type(), HsmsMessageType::SelectReq);
        assert!(!back.is_data_message());
    }

    #[test]
    fn select_response_wire_roundtrip() {
        let primary_h = [0x00, 0x05, 0x00, 0x00, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        let primary = HsmsMessage::of(&primary_h).unwrap();
        let rsp = build_select_response(&primary, SelectStatus::Success).unwrap();
        let w = rsp.to_wire().unwrap();
        let back = HsmsMessage::from_wire(&w).unwrap();
        assert_eq!(back.message_type(), HsmsMessageType::SelectRsp);
        assert_eq!(back.header10_bytes()[3], 0x00);
        assert_eq!(back.header10_bytes()[6], 0xAA);
    }

    #[test]
    fn data_message_with_body_roundtrip() {
        let header = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
        ];
        let body = Secs2::list([
            Secs2::ascii("ABC").unwrap(),
            Secs2::int4([1, 2, 3]).unwrap(),
        ])
        .unwrap();
        let m = HsmsMessage::of_with_body(&header, body).unwrap();
        let w = m.to_wire().unwrap();
        let msg_len = u32::from_be_bytes([w[0], w[1], w[2], w[3]]) as usize;
        assert!(msg_len > 10);
        assert_eq!(w.len(), 4 + msg_len);

        let back = HsmsMessage::from_wire(&w).unwrap();
        assert!(back.is_data_message());
        assert_eq!(back.get_stream(), 1);
        assert_eq!(back.get_function(), 1);
        assert!(back.wbit());
        assert_eq!(back.secs2().get_ascii_at(&[0]).unwrap(), "ABC");
        assert_eq!(back.secs2().get_int_at(&[1, 1]).unwrap(), 2);
    }

    #[test]
    fn length_below_ten_rejected() {
        let bad = [0u8, 0, 0, 5, 0, 0, 0, 0, 0];
        assert_eq!(
            HsmsMessage::from_wire(&bad),
            Err(Error::LengthBytesLowerThanTen)
        );
    }

    #[test]
    fn truncated_frame() {
        let header = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01];
        let m = HsmsMessage::of(&header).unwrap();
        let w = m.to_wire().unwrap();
        assert_eq!(
            HsmsMessage::from_wire(&w[..10]),
            Err(Error::Truncated)
        );
    }

    #[test]
    fn build_from_parts_control() {
        let header = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01];
        let m = build_from_parts(&header, &[]).unwrap();
        assert_eq!(m.message_type(), HsmsMessageType::SelectReq);
    }
}
