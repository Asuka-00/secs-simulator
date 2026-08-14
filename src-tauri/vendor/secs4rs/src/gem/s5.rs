//! GEM Stream-5 alarm helpers.
//!
//! Source: `AbstractGem.S5f2` / `S5f4` + S5F1 body shape from secs4java8 README
//! (`L <B ALCD> <U* ALID> <A ALTX>`).

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::{self, Secs2};
use crate::SecsMessage;

use super::ack::Ackc5;
use super::error::{expect_reply, GemError};

/// S5F1 body: `L <ALCD> <ALID> <ALTX>`.
pub fn s5f1_body(alcd: Secs2, alid: Secs2, altx: Secs2) -> secs2::Result<Secs2> {
    Secs2::list([alcd, alid, altx])
}

/// Build common S5F1 body: ALCD Binary, ALID U4, ALTX ASCII.
pub fn s5f1_body_parts(alcd: u8, alid: u32, altx: &str) -> secs2::Result<Secs2> {
    s5f1_body(
        Secs2::binary([alcd])?,
        Secs2::uint4([alid])?,
        Secs2::ascii(altx)?,
    )
}

/// Send S5F1 Alarm Report (W-bit) → parse S5F2 → [`Ackc5`].
pub fn s5f1(comm: &HsmsSsCommunicator, body: Secs2) -> Result<Ackc5, GemError> {
    let reply = comm.send_data(5, 1, true, body)?;
    let m = expect_reply(reply, 5, 2)?;
    Ok(Ackc5::from_secs2(m.secs2())?)
}

/// Convenience: ALCD / ALID / ALTX → S5F1 → ACKC5.
pub fn s5f1_alarm(
    comm: &HsmsSsCommunicator,
    alcd: u8,
    alid: u32,
    altx: &str,
) -> Result<Ackc5, GemError> {
    let body = s5f1_body_parts(alcd, alid, altx)?;
    s5f1(comm, body)
}

/// S5F2 Alarm Report Acknowledge (`AbstractGem.S5f2`).
pub fn s5f2(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc5: Ackc5,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 5, 2, false, ackc5.secs2())?;
    Ok(true)
}

/// S5F3 body: `L <ALED> <ALID…|empty>`.
///
/// `alids` empty list means all alarms (common host path).
pub fn s5f3_body(aled: Secs2, alids: impl IntoIterator<Item = Secs2>) -> secs2::Result<Secs2> {
    // SEMI: often L <ALED> <ALID> for single, or L <ALED> <L ALIDs>.
    // Source library only exposes S5F4 reply; body here matches common single-ALID list form:
    // L <ALED> <L <ALID…>>.
    let list = Secs2::list(alids)?;
    Secs2::list([aled, list])
}

/// Build S5F3 with ALED byte + U4 ALID list.
pub fn s5f3_body_parts(aled: u8, alids: &[u32]) -> secs2::Result<Secs2> {
    let ids: Result<Vec<_>, _> = alids.iter().map(|id| Secs2::uint4([*id])).collect();
    s5f3_body(Secs2::binary([aled])?, ids?)
}

/// Send S5F3 Enable/Disable Alarms (W-bit) → parse S5F4 → [`Ackc5`].
pub fn s5f3(comm: &HsmsSsCommunicator, body: Secs2) -> Result<Ackc5, GemError> {
    let reply = comm.send_data(5, 3, true, body)?;
    let m = expect_reply(reply, 5, 4)?;
    Ok(Ackc5::from_secs2(m.secs2())?)
}

/// S5F4 Enable/Disable Alarm Acknowledge (`AbstractGem.S5f4`).
pub fn s5f4(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc5: Ackc5,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 5, 4, false, ackc5.secs2())?;
    Ok(true)
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
    fn s5f1_body_shape_readme() {
        // secs4java8 README: ALCD 0x81, ALID 1001, ALTX "ON FIRE"
        let body = s5f1_body_parts(0x81, 1001, "ON FIRE").unwrap();
        assert_eq!(body.size(), 3);
        assert_eq!(body.get_byte_at(&[0, 0]).unwrap(), 0x81);
        assert_eq!(body.get_long_at(&[1, 0]).unwrap(), 1001);
        assert_eq!(body.get_ascii_at(&[2]).unwrap(), "ON FIRE");
    }

    #[test]
    fn s5f1_2_alarm_report_roundtrip() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 5);
            assert_eq!(primary.get_function(), 1);
            assert!(primary.wbit());
            assert_eq!(primary.secs2().get_byte_at(&[0, 0]).unwrap(), 0x81);
            assert_eq!(primary.secs2().get_long_at(&[1, 0]).unwrap(), 1001);
            assert_eq!(primary.secs2().get_ascii_at(&[2]).unwrap(), "ON FIRE");
            assert!(s5f2(&pass, &primary, Ackc5::Ok).unwrap());
        });

        let ack = s5f1_alarm(&active, 0x81, 1001, "ON FIRE").unwrap();
        assert_eq!(ack, Ackc5::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s5f3_4_enable_alarm_roundtrip() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 5);
            assert_eq!(primary.get_function(), 3);
            assert!(primary.wbit());
            // ALED enable bit often 0x80
            assert_eq!(primary.secs2().get_byte_at(&[0, 0]).unwrap(), 0x80);
            let alids = primary.secs2().get_item(&[1]).unwrap();
            assert_eq!(alids.size(), 1);
            assert_eq!(alids.get_long_at(&[0, 0]).unwrap(), 1001);
            assert!(s5f4(&pass, &primary, Ackc5::Ok).unwrap());
        });

        let body = s5f3_body_parts(0x80, &[1001]).unwrap();
        let ack = s5f3(&active, body).unwrap();
        assert_eq!(ack, Ackc5::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s5f2_no_wbit_returns_false() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert!(!primary.wbit());
            assert!(!s5f2(&pass, &primary, Ackc5::Ok).unwrap());
        });

        let body = s5f1_body_parts(0x01, 1, "x").unwrap();
        let reply = active.send_data(5, 1, false, body).unwrap();
        assert!(reply.is_none());

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s5f1_unexpected_function_errors() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            pass.send_data_reply(&primary, 5, 99, false, Ackc5::Ok.secs2())
                .unwrap();
        });

        let err = s5f1_alarm(&active, 0x81, 1, "x").unwrap_err();
        match err {
            GemError::UnexpectedReply {
                expected_stream: 5,
                expected_function: 2,
                got_function: Some(99),
                ..
            } => {}
            other => panic!("expected UnexpectedReply S5F2, got {other:?}"),
        }

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
