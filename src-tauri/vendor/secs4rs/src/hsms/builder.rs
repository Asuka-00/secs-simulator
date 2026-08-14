//! HSMS control/data request/response builders.
//!
//! Source: `AbstractHsmsMessageBuilder` + `AbstractHsmsSsMessageBuilder`.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::secs2::Secs2;

use super::message::HsmsMessage;
use super::message_type::HsmsMessageType;
use super::status::{DeselectStatus, RejectReason, SelectStatus};
use super::Result;

/// Auto-increment system-bytes serial (`AbstractSecsMessageBuilder._autoNum`).
#[derive(Debug, Default)]
pub struct SystemBytesCounter {
    auto_num: AtomicI32,
}

impl SystemBytesCounter {
    pub fn new() -> Self {
        Self {
            auto_num: AtomicI32::new(0),
        }
    }

    /// Next 4 system-bytes (host: high 2 = 0; equip: high 2 = session_id BE).
    ///
    /// Serial is low 2 bytes after atomic increment (parity with Interlocked.Increment).
    pub fn next(&self, is_equip: bool, session_id: i32) -> [u8; 4] {
        let num = self.auto_num.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        let lo = ((num >> 8) as u8, num as u8);
        if is_equip {
            [(session_id >> 8) as u8, session_id as u8, lo.0, lo.1]
        } else {
            [0x00, 0x00, lo.0, lo.1]
        }
    }
}

fn control_req_device(device: [u8; 2], msg_type: HsmsMessageType, sys: [u8; 4]) -> Result<HsmsMessage> {
    let h = [
        device[0],
        device[1],
        0x00,
        0x00,
        msg_type.p_type(),
        msg_type.s_type(),
        sys[0],
        sys[1],
        sys[2],
        sys[3],
    ];
    HsmsMessage::of(&h)
}

fn control_req(msg_type: HsmsMessageType, sys: [u8; 4]) -> Result<HsmsMessage> {
    // SS control req: device-bytes fixed 0xFF,0xFF (AbstractHsmsSsMessageBuilder).
    control_req_device([0xFF, 0xFF], msg_type, sys)
}

/// Build SELECT.req (SS: device-bytes 0xFFFF).
pub fn build_select_request(sys: [u8; 4]) -> Result<HsmsMessage> {
    control_req(HsmsMessageType::SelectReq, sys)
}

/// Build SELECT.req for HSMS-GS (device-bytes = session-id BE).
///
/// Source: `AbstractHsmsGsMessageBuilder.BuildSelectRequest`.
pub fn build_select_request_gs(session_id: i32, sys: [u8; 4]) -> Result<HsmsMessage> {
    let device = [(session_id >> 8) as u8, session_id as u8];
    control_req_device(device, HsmsMessageType::SelectReq, sys)
}

/// Build LINKTEST.req (device-bytes 0xFFFF; SS and GS linktest).
pub fn build_linktest_request(sys: [u8; 4]) -> Result<HsmsMessage> {
    control_req(HsmsMessageType::LinktestReq, sys)
}

/// Build SEPARATE.req (SS: device-bytes 0xFFFF).
pub fn build_separate_request(sys: [u8; 4]) -> Result<HsmsMessage> {
    control_req(HsmsMessageType::SeparateReq, sys)
}

/// Build SEPARATE.req for HSMS-GS (device-bytes = session-id BE).
pub fn build_separate_request_gs(session_id: i32, sys: [u8; 4]) -> Result<HsmsMessage> {
    let device = [(session_id >> 8) as u8, session_id as u8];
    control_req_device(device, HsmsMessageType::SeparateReq, sys)
}

/// Build DESELECT.req (SS: device-bytes 0xFFFF).
pub fn build_deselect_request(sys: [u8; 4]) -> Result<HsmsMessage> {
    control_req(HsmsMessageType::DeselectReq, sys)
}

/// Build DESELECT.req for HSMS-GS (device-bytes = session-id BE).
pub fn build_deselect_request_gs(session_id: i32, sys: [u8; 4]) -> Result<HsmsMessage> {
    let device = [(session_id >> 8) as u8, session_id as u8];
    control_req_device(device, HsmsMessageType::DeselectReq, sys)
}

/// Build primary DATA message (new system-bytes).
///
/// Header: session BE, stream|wbit, func, p/s=0/0, sys[4].
pub fn build_data_message(
    session_id: i32,
    strm: i32,
    func: i32,
    wbit: bool,
    body: Secs2,
    sys: [u8; 4],
) -> Result<HsmsMessage> {
    let mut stream_b = (strm as u8) & 0x7F;
    if wbit {
        stream_b |= 0x80;
    }
    let h = [
        (session_id >> 8) as u8,
        session_id as u8,
        stream_b,
        func as u8,
        HsmsMessageType::Data.p_type(),
        HsmsMessageType::Data.s_type(),
        sys[0],
        sys[1],
        sys[2],
        sys[3],
    ];
    HsmsMessage::of_with_body(&h, body)
}

/// Build reply DATA message (reuses primary system-bytes).
pub fn build_data_reply(
    session_id: i32,
    primary: &HsmsMessage,
    strm: i32,
    func: i32,
    wbit: bool,
    body: Secs2,
) -> Result<HsmsMessage> {
    build_data_reply_from_header(session_id, &primary.header10_bytes(), strm, func, wbit, body)
}

/// Build reply DATA from primary header-10-bytes (system-bytes reuse).
///
/// Used by entity SxF0 path when only `SecsMessage` surface is available.
pub fn build_data_reply_from_header(
    session_id: i32,
    primary_header: &[u8; 10],
    strm: i32,
    func: i32,
    wbit: bool,
    body: Secs2,
) -> Result<HsmsMessage> {
    let bs = primary_header;
    let mut stream_b = (strm as u8) & 0x7F;
    if wbit {
        stream_b |= 0x80;
    }
    let h = [
        (session_id >> 8) as u8,
        session_id as u8,
        stream_b,
        func as u8,
        HsmsMessageType::Data.p_type(),
        HsmsMessageType::Data.s_type(),
        bs[6],
        bs[7],
        bs[8],
        bs[9],
    ];
    HsmsMessage::of_with_body(&h, body)
}

/// Build SELECT.rsp from primary request (preserves session + system bytes).
pub fn build_select_response(primary: &HsmsMessage, status: SelectStatus) -> Result<HsmsMessage> {
    let bs = primary.header10_bytes();
    let h = [
        bs[0],
        bs[1],
        0x00,
        status.status_code(),
        HsmsMessageType::SelectRsp.p_type(),
        HsmsMessageType::SelectRsp.s_type(),
        bs[6],
        bs[7],
        bs[8],
        bs[9],
    ];
    HsmsMessage::of(&h)
}

/// Build DESELECT.rsp from primary request.
pub fn build_deselect_response(
    primary: &HsmsMessage,
    status: DeselectStatus,
) -> Result<HsmsMessage> {
    let bs = primary.header10_bytes();
    let h = [
        bs[0],
        bs[1],
        0x00,
        status.status_code(),
        HsmsMessageType::DeselectRsp.p_type(),
        HsmsMessageType::DeselectRsp.s_type(),
        bs[6],
        bs[7],
        bs[8],
        bs[9],
    ];
    HsmsMessage::of(&h)
}

/// Build LINKTEST.rsp from primary request.
pub fn build_linktest_response(primary: &HsmsMessage) -> Result<HsmsMessage> {
    let bs = primary.header10_bytes();
    let h = [
        bs[0],
        bs[1],
        0x00,
        0x00,
        HsmsMessageType::LinktestRsp.p_type(),
        HsmsMessageType::LinktestRsp.s_type(),
        bs[6],
        bs[7],
        bs[8],
        bs[9],
    ];
    HsmsMessage::of(&h)
}

/// Build REJECT.req referencing the offending message.
pub fn build_reject_request(
    reference: &HsmsMessage,
    reason: RejectReason,
) -> Result<HsmsMessage> {
    let bs = reference.header10_bytes();
    // byte2 echoes offending P-type (byte4) or S-type (byte5).
    let b2 = if reason == RejectReason::NotSupportTypeP {
        bs[4]
    } else {
        bs[5]
    };
    let h = [
        bs[0],
        bs[1],
        b2,
        reason.reason_code(),
        HsmsMessageType::RejectReq.p_type(),
        HsmsMessageType::RejectReq.s_type(),
        bs[6],
        bs[7],
        bs[8],
        bs[9],
    ];
    HsmsMessage::of(&h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// primary SELECT_REQ: device=0x0005, p/s=0/1, sysbytes=AA BB CC DD
    fn primary_header() -> [u8; 10] {
        [0x00, 0x05, 0x00, 0x00, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD]
    }

    #[test]
    fn hsms_build_select_response() {
        let primary = HsmsMessage::of(&primary_header()).unwrap();
        let rsp = build_select_response(&primary, SelectStatus::Success).unwrap();
        assert_eq!(rsp.message_type(), HsmsMessageType::SelectRsp);
        assert_eq!(SelectStatus::from_message(&rsp), SelectStatus::Success);
        let rh = rsp.header10_bytes();
        assert_eq!(rh[6], 0xAA);
        assert_eq!(rh[7], 0xBB);
        assert_eq!(rh[8], 0xCC);
        assert_eq!(rh[9], 0xDD);
        assert_eq!(rh[3], 0x00); // SUCCESS
    }

    #[test]
    fn hsms_build_deselect_response() {
        let primary = HsmsMessage::of(&primary_header()).unwrap();
        let rsp = build_deselect_response(&primary, DeselectStatus::Success).unwrap();
        assert_eq!(rsp.message_type(), HsmsMessageType::DeselectRsp);
        assert_eq!(DeselectStatus::from_message(&rsp), DeselectStatus::Success);
    }

    #[test]
    fn hsms_build_linktest_response() {
        let primary = HsmsMessage::of(&primary_header()).unwrap();
        let rsp = build_linktest_response(&primary).unwrap();
        assert_eq!(rsp.message_type(), HsmsMessageType::LinktestRsp);
        let rh = rsp.header10_bytes();
        assert_eq!(rh[6], 0xAA);
        assert_eq!(rh[9], 0xDD);
    }

    #[test]
    fn hsms_build_reject_request() {
        let primary = HsmsMessage::of(&primary_header()).unwrap();
        let rej = build_reject_request(&primary, RejectReason::NotSupportTypeS).unwrap();
        assert_eq!(rej.message_type(), HsmsMessageType::RejectReq);
        // reason NOT_SUPPORT_TYPE_S = 1 in byte[3]
        assert_eq!(rej.header10_bytes()[3], 0x01);
        // byte2 echoes S-type of reference (SELECT_REQ s=1)
        assert_eq!(rej.header10_bytes()[2], 0x01);
    }

    #[test]
    fn hsms_build_select_request() {
        let sys = [0x00, 0x00, 0x12, 0x34];
        let req = build_select_request(sys).unwrap();
        assert_eq!(req.message_type(), HsmsMessageType::SelectReq);
        let h = req.header10_bytes();
        assert_eq!(&h[0..2], &[0xFF, 0xFF]);
        assert_eq!(&h[4..6], &[0x00, 0x01]);
        assert_eq!(&h[6..10], &sys);
        assert_eq!(req.session_id(), -1);
    }

    #[test]
    fn hsms_build_select_request_gs_session_device() {
        let sys = [0x00, 0x00, 0x00, 0x07];
        let req = build_select_request_gs(0x0A0B, sys).unwrap();
        assert_eq!(req.message_type(), HsmsMessageType::SelectReq);
        let h = req.header10_bytes();
        // GS: device-bytes = session id, not 0xFFFF
        assert_eq!(&h[0..2], &[0x0A, 0x0B]);
        assert_eq!(&h[4..6], &[0x00, 0x01]);
        assert_eq!(&h[6..10], &sys);
        // Control with non-FFFF session bytes keeps raw session id
        assert_eq!(req.session_id(), 0x0A0B);
    }

    #[test]
    fn hsms_system_bytes_counter_host_equip() {
        let c = SystemBytesCounter::new();
        let h1 = c.next(false, 10);
        assert_eq!(h1, [0x00, 0x00, 0x00, 0x01]);
        let h2 = c.next(false, 10);
        assert_eq!(h2, [0x00, 0x00, 0x00, 0x02]);
        let e = SystemBytesCounter::new().next(true, 0x0A0B);
        assert_eq!(e, [0x0A, 0x0B, 0x00, 0x01]);
    }

    #[test]
    fn hsms_build_data_message_wbit() {
        let body = Secs2::ascii("AB").unwrap();
        let msg = build_data_message(10, 1, 1, true, body, [0, 0, 0, 5]).unwrap();
        assert!(msg.is_data_message());
        assert!(msg.wbit());
        assert_eq!(msg.get_stream(), 1);
        assert_eq!(msg.get_function(), 1);
        assert_eq!(msg.session_id(), 10);
        assert_eq!(msg.system_bytes_key(), 5);
        assert_eq!(msg.secs2().get_ascii().unwrap(), "AB");
        let h = msg.header10_bytes();
        assert_eq!(h[2], 0x81);
    }

    #[test]
    fn hsms_build_data_reply_reuses_sysbytes() {
        let primary = build_data_message(
            10,
            1,
            1,
            true,
            Secs2::empty(),
            [0xAA, 0xBB, 0xCC, 0xDD],
        )
        .unwrap();
        let reply = build_data_reply(
            10,
            &primary,
            1,
            2,
            false,
            Secs2::ascii("OK").unwrap(),
        )
        .unwrap();
        assert!(!reply.wbit());
        assert_eq!(reply.get_function(), 2);
        assert_eq!(reply.system_bytes_key(), primary.system_bytes_key());
        assert_eq!(reply.header10_bytes()[6], 0xAA);
        assert_eq!(reply.header10_bytes()[9], 0xDD);
    }
}
