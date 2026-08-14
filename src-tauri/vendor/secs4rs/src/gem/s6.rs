//! GEM Stream-6 data collection / event report helpers.
//!
//! Source: `AbstractGem.S6f*` + S6F11 body shape from GEM usage
//! (`L <DATAID> <CEID> <L reports…>`; reports = `L <RPTID> <L V…>`).

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::{self, Secs2};
use crate::SecsMessage;

use super::ack::{Ackc6, Grant6};
use super::dynamic::{DynamicCollectionEvent, DynamicReport};
use super::error::{expect_reply, GemError};

/// One S6F11 report entry: `L <RPTID> <L values…>`.
pub fn s6f11_report(
    rptid: Secs2,
    values: impl IntoIterator<Item = Secs2>,
) -> secs2::Result<Secs2> {
    let vals = Secs2::list(values)?;
    Secs2::list([rptid, vals])
}

/// S6F11 body: `L <DATAID> <CEID> <L reports…>`.
pub fn s6f11_body(
    data_id: Secs2,
    ceid: Secs2,
    reports: impl IntoIterator<Item = Secs2>,
) -> secs2::Result<Secs2> {
    let reports_list = Secs2::list(reports)?;
    Secs2::list([data_id, ceid, reports_list])
}

/// Send S6F11 Event Report (W-bit) → parse S6F12 → [`Ackc6`].
pub fn s6f11(comm: &HsmsSsCommunicator, body: Secs2) -> Result<Ackc6, GemError> {
    let reply = comm.send_data(6, 11, true, body)?;
    let m = expect_reply(reply, 6, 12)?;
    Ok(Ackc6::from_secs2(m.secs2())?)
}

/// Build body + send S6F11.
pub fn s6f11_event(
    comm: &HsmsSsCommunicator,
    data_id: Secs2,
    ceid: Secs2,
    reports: impl IntoIterator<Item = Secs2>,
) -> Result<Ackc6, GemError> {
    let body = s6f11_body(data_id, ceid, reports)?;
    s6f11(comm, body)
}

/// Empty-report S6F11 (common equipment notify path).
pub fn s6f11_empty(
    comm: &HsmsSsCommunicator,
    data_id: Secs2,
    ceid: Secs2,
) -> Result<Ackc6, GemError> {
    s6f11_event(comm, data_id, ceid, [])
}

/// Shared Stream-6 ACKC6 reply helper (`S6fx`).
fn s6fx(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    func: i32,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 6, func, false, ackc6.secs2())?;
    Ok(true)
}

/// S6F2 Trace Data Acknowledge.
pub fn s6f2(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 2, ackc6)
}

/// S6F4 Discrete Variable Data Send Acknowledge.
pub fn s6f4(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 4, ackc6)
}

/// S6F6 Multi-block Data Grant.
pub fn s6f6(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    grant6: Grant6,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 6, 6, false, grant6.secs2())?;
    Ok(true)
}

/// S6F10 Formatted Variable Acknowledge.
pub fn s6f10(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 10, ackc6)
}

/// S6F12 Event Report Acknowledge (reply to S6F11).
pub fn s6f12(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 12, ackc6)
}

/// S6F14 Annotated Event Report Acknowledge.
pub fn s6f14(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 14, ackc6)
}

/// S6F26 Notification Report Acknowledge.
pub fn s6f26(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc6: Ackc6,
) -> Result<bool, GemError> {
    s6fx(comm, primary, 26, ackc6)
}

/// S6F15 Event Report Request (body = CEID).
pub fn s6f15(
    comm: &HsmsSsCommunicator,
    ce: &DynamicCollectionEvent,
) -> Result<Option<HsmsMessage>, GemError> {
    Ok(comm.send_data(6, 15, true, ce.collection_event_id().clone())?)
}

/// S6F17 Annotated Event Report Request (body = CEID).
pub fn s6f17(
    comm: &HsmsSsCommunicator,
    ce: &DynamicCollectionEvent,
) -> Result<Option<HsmsMessage>, GemError> {
    Ok(comm.send_data(6, 17, true, ce.collection_event_id().clone())?)
}

/// S6F19 Individual Report Request (body = RPTID).
pub fn s6f19(
    comm: &HsmsSsCommunicator,
    report: &DynamicReport,
) -> Result<Option<HsmsMessage>, GemError> {
    Ok(comm.send_data(6, 19, true, report.report_id().clone())?)
}

/// S6F21 Annotated Individual Report Request (body = RPTID).
pub fn s6f21(
    comm: &HsmsSsCommunicator,
    report: &DynamicReport,
) -> Result<Option<HsmsMessage>, GemError> {
    Ok(comm.send_data(6, 21, true, report.report_id().clone())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::{HsmsCommunicateState, HsmsConnectionMode};
    use crate::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn open_pair() -> (Arc<HsmsSsCommunicator>, Arc<HsmsSsCommunicator>) {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(5.0);
        a_cfg.timeout().set_t6(5.0);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));

        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || p_arc.open_passive().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        active.open_active().unwrap();
        p.join().unwrap();
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        (active, passive)
    }

    #[test]
    fn s6f11_body_shape() {
        let data_id = Secs2::uint4([1]).unwrap();
        let ceid = Secs2::uint2([100]).unwrap();
        let rptid = Secs2::uint4([7]).unwrap();
        let v = Secs2::ascii("OK").unwrap();
        let report = s6f11_report(rptid, [v]).unwrap();
        let body = s6f11_body(data_id, ceid, [report]).unwrap();

        // L3: DATAID, CEID, reports
        assert_eq!(body.size(), 3);
        assert_eq!(body.get_long_at(&[0, 0]).unwrap(), 1);
        assert_eq!(body.get_long_at(&[1, 0]).unwrap(), 100);
        let reports = body.get_item(&[2]).unwrap();
        assert_eq!(reports.size(), 1);
        let r0 = reports.get_item(&[0]).unwrap();
        assert_eq!(r0.get_long_at(&[0, 0]).unwrap(), 7);
        assert_eq!(
            r0.get_item(&[1]).unwrap().get_item(&[0]).unwrap().get_ascii().unwrap(),
            "OK"
        );
    }

    #[test]
    fn s6f11_empty_body_shape() {
        let body = s6f11_body(
            Secs2::uint4([9]).unwrap(),
            Secs2::uint2([1]).unwrap(),
            [],
        )
        .unwrap();
        assert_eq!(body.size(), 3);
        assert_eq!(body.get_item(&[2]).unwrap().size(), 0);
    }

    #[test]
    fn s6f11_12_event_report_roundtrip() {
        let (active, passive) = open_pair();

        // Host (passive) waits for S6F11 and replies S6F12 ACKC6.OK
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 6);
            assert_eq!(primary.get_function(), 11);
            assert!(primary.wbit());
            // body: L <DATAID=42> <CEID=1000> <L empty>
            assert_eq!(primary.secs2().get_long_at(&[0, 0]).unwrap(), 42);
            assert_eq!(primary.secs2().get_long_at(&[1, 0]).unwrap(), 1000);
            assert_eq!(primary.secs2().get_item(&[2]).unwrap().size(), 0);
            assert!(s6f12(&pass, &primary, Ackc6::Ok).unwrap());
        });

        // Equip (active) fires S6F11 empty report
        let ack = s6f11_empty(
            &active,
            Secs2::uint4([42]).unwrap(),
            Secs2::uint2([1000]).unwrap(),
        )
        .unwrap();
        assert_eq!(ack, Ackc6::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s6f11_12_with_reports_roundtrip() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_function(), 11);
            let reports = primary.secs2().get_item(&[2]).unwrap();
            assert_eq!(reports.size(), 1);
            assert!(s6f12(&pass, &primary, Ackc6::Ok).unwrap());
        });

        let report = s6f11_report(
            Secs2::uint4([1]).unwrap(),
            [Secs2::uint4([99]).unwrap(), Secs2::ascii("V").unwrap()],
        )
        .unwrap();
        let ack = s6f11_event(
            &active,
            Secs2::uint4([1]).unwrap(),
            Secs2::uint4([5]).unwrap(),
            [report],
        )
        .unwrap();
        assert_eq!(ack, Ackc6::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s6f12_no_wbit_returns_false() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            // primary has no W-bit
            assert!(!primary.wbit());
            assert!(!s6f12(&pass, &primary, Ackc6::Ok).unwrap());
        });

        // Send S6F11 without W-bit (direct send_data)
        let body = s6f11_body(
            Secs2::uint4([1]).unwrap(),
            Secs2::uint2([1]).unwrap(),
            [],
        )
        .unwrap();
        let reply = active.send_data(6, 11, false, body).unwrap();
        assert!(reply.is_none());

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s6f15_request_ceid_body() {
        let (active, passive) = open_pair();
        let ce = DynamicCollectionEvent::new(Some("e".into()), Secs2::uint2([77]).unwrap());

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 6);
            assert_eq!(primary.get_function(), 15);
            assert!(primary.wbit());
            assert_eq!(primary.secs2().get_long_at(&[0]).unwrap(), 77);
            // reply header-only S6F16 (not fully modeled) as empty body
            pass.send_data_reply(&primary, 6, 16, false, Secs2::empty())
                .unwrap();
        });

        let reply = s6f15(&active, &ce).unwrap().expect("S6F16");
        assert_eq!(reply.get_stream(), 6);
        assert_eq!(reply.get_function(), 16);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s6f11_unexpected_function_errors() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            // wrong function reply (S6F99 instead of S6F12)
            pass.send_data_reply(&primary, 6, 99, false, Ackc6::Ok.secs2())
                .unwrap();
        });

        let err = s6f11_empty(
            &active,
            Secs2::uint4([1]).unwrap(),
            Secs2::uint2([1]).unwrap(),
        )
        .unwrap_err();
        match err {
            GemError::UnexpectedReply {
                expected_stream: 6,
                expected_function: 12,
                got_function: Some(99),
                ..
            } => {}
            other => panic!("expected UnexpectedReply S6F12, got {other:?}"),
        }

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
