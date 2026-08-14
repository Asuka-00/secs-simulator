//! GEM Stream-2 helpers over an HSMS-SS session.
//!
//! Source: `AbstractGem` S2F17/18/31/32 + S2F33…38.
//! Free functions on [`HsmsSsCommunicator`] (no AbstractGem inheritance).

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::Secs2;
use crate::SecsMessage;

use super::ack::{Drack, Erack, Lrack, TiAck};
use super::clock::Clock;
use super::config::GemConfig;
use super::dynamic_config::DynamicEventReportConfig;
use super::error::{expect_reply, GemError};
use super::Ceed;

/// Alias for historical `S2Error` name.
pub type S2Error = GemError;

/// S2F17 Date and Time Request → parse S2F18 into [`Clock`].
pub fn s2f17(comm: &HsmsSsCommunicator) -> Result<Clock, S2Error> {
    let reply = comm.send_data(2, 17, true, Secs2::empty())?;
    let m = expect_reply(reply, 2, 18)?;
    Ok(Clock::from_secs2(m.secs2())?)
}

/// S2F18 Date and Time Data reply with given clock (encoded per config).
pub fn s2f18(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    config: &GemConfig,
    clock: &Clock,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    let body = config.clock_secs2(clock)?;
    comm.send_data_reply(primary, 2, 18, false, body)?;
    Ok(true)
}

/// S2F18 reply with pre-encoded SECS-II body.
pub fn s2f18_body(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    body: Secs2,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 18, false, body)?;
    Ok(true)
}

/// S2F31 Date and Time Set Request → S2F32 TIACK.
pub fn s2f31(
    comm: &HsmsSsCommunicator,
    config: &GemConfig,
    clock: &Clock,
) -> Result<TiAck, S2Error> {
    let body = config.clock_secs2(clock)?;
    let reply = comm.send_data(2, 31, true, body)?;
    let m = expect_reply(reply, 2, 32)?;
    Ok(TiAck::from_secs2(m.secs2())?)
}

/// S2F32 Date and Time Set Acknowledge.
pub fn s2f32(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    tiack: TiAck,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 32, false, tiack.secs2())?;
    Ok(true)
}

/// Send S2F33 body and parse S2F34 → [`Drack`] (`S2f33Inner`).
pub fn s2f33(
    comm: &HsmsSsCommunicator,
    body: Secs2,
) -> Result<Drack, S2Error> {
    let reply = comm.send_data(2, 33, true, body)?;
    let m = expect_reply(reply, 2, 34)?;
    Ok(Drack::from_secs2(m.secs2())?)
}

/// `S2f33Define` using config body + auto DATAID.
pub fn s2f33_define(
    comm: &HsmsSsCommunicator,
    config: &DynamicEventReportConfig,
) -> Result<Drack, S2Error> {
    let data_id = config.auto_data_id_u4()?;
    let body = config.s2f33_define_body(data_id)?;
    s2f33(comm, body)
}

/// `S2f33DeleteAll`.
pub fn s2f33_delete_all(
    comm: &HsmsSsCommunicator,
    config: &DynamicEventReportConfig,
) -> Result<Drack, S2Error> {
    let data_id = config.auto_data_id_u4()?;
    let body = config.s2f33_delete_all_body(data_id)?;
    s2f33(comm, body)
}

/// Reply S2F34 with DRACK (`S2f34`). Returns `Ok(false)` if primary has no W-bit.
pub fn s2f34(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    drack: Drack,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 34, false, drack.secs2())?;
    Ok(true)
}

/// Send S2F35 body and parse S2F36 → [`Lrack`].
pub fn s2f35(comm: &HsmsSsCommunicator, body: Secs2) -> Result<Lrack, S2Error> {
    let reply = comm.send_data(2, 35, true, body)?;
    let m = expect_reply(reply, 2, 36)?;
    Ok(Lrack::from_secs2(m.secs2())?)
}

/// `S2f35` from config links.
pub fn s2f35_link(
    comm: &HsmsSsCommunicator,
    config: &DynamicEventReportConfig,
) -> Result<Lrack, S2Error> {
    let data_id = config.auto_data_id_u4()?;
    let body = config.s2f35_body(data_id)?;
    s2f35(comm, body)
}

/// Reply S2F36 with LRACK.
pub fn s2f36(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    lrack: Lrack,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 36, false, lrack.secs2())?;
    Ok(true)
}

/// Send S2F37 body and parse S2F38 → [`Erack`].
pub fn s2f37(comm: &HsmsSsCommunicator, body: Secs2) -> Result<Erack, S2Error> {
    let reply = comm.send_data(2, 37, true, body)?;
    let m = expect_reply(reply, 2, 38)?;
    Ok(Erack::from_secs2(m.secs2())?)
}

/// `S2f37Enable` from config CE list.
pub fn s2f37_enable(
    comm: &HsmsSsCommunicator,
    config: &DynamicEventReportConfig,
) -> Result<Erack, S2Error> {
    let body = config.s2f37_enable_body(&Ceed::Enable.secs2())?;
    s2f37(comm, body)
}

/// Reply S2F38 with ERACK.
pub fn s2f38(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    erack: Erack,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 38, false, erack.secs2())?;
    Ok(true)
}

/// S2F40 Multi-block Grant (`AbstractGem.S2f40`).
pub fn s2f40(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    grant: super::ack::Grant,
) -> Result<bool, S2Error> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 2, 40, false, grant.secs2())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gem::clock::{ClockType, LocalDateTime};
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
    fn s2f17_18_date_time_a16() {
        let (active, passive) = open_pair();
        let pcfg = GemConfig::new();
        pcfg.set_clock_type(ClockType::A16);
        let clock = Clock::from_local(LocalDateTime {
            year: 2026,
            month: 8,
            day: 5,
            hour: 12,
            minute: 34,
            second: 56,
            hundredths: 78,
        });

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 2);
            assert_eq!(primary.get_function(), 17);
            assert!(s2f18(&pass, &primary, &pcfg, &clock).unwrap());
        });

        let got = s2f17(&active).unwrap();
        assert_eq!(got.to_local_date_time().year, 2026);
        assert_eq!(got.to_local_date_time().minute, 34);
        assert_eq!(got.to_local_date_time().hundredths, 78);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f17_18_date_time_a12() {
        let (active, passive) = open_pair();
        let pcfg = GemConfig::new();
        pcfg.set_clock_type(ClockType::A12);
        let clock = Clock::from_local(LocalDateTime::new(2026, 3, 15, 9, 8, 7));

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert!(s2f18(&pass, &primary, &pcfg, &clock).unwrap());
        });

        let got = s2f17(&active).unwrap();
        // A12 year expand depends on current century; month/day/time exact
        assert_eq!(got.to_local_date_time().month, 3);
        assert_eq!(got.to_local_date_time().day, 15);
        assert_eq!(got.to_local_date_time().hour, 9);
        assert_eq!(got.to_local_date_time().second, 7);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f31_32_time_set() {
        let (active, passive) = open_pair();
        let acfg = GemConfig::new();
        acfg.set_clock_type(ClockType::A16);
        let clock = Clock::from_local(LocalDateTime::new(2025, 1, 2, 3, 4, 5));

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_function(), 31);
            // body is A16 ascii
            assert_eq!(primary.secs2().get_ascii().unwrap().len(), 16);
            assert!(s2f32(&pass, &primary, TiAck::Ok).unwrap());
        });

        let ack = s2f31(&active, &acfg, &clock).unwrap();
        assert_eq!(ack, TiAck::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f33_34_define_roundtrip_ok() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 2);
            assert_eq!(primary.get_function(), 33);
            assert!(primary.wbit());
            // L <DATAID> <L reports>
            assert_eq!(primary.secs2().size(), 2);
            assert!(s2f34(&pass, &primary, Drack::Ok).unwrap());
        });

        let cfg = DynamicEventReportConfig::new();
        cfg.add_define_report(101, Some("R1".into()), &[1, 2])
            .unwrap();
        let drack = s2f33_define(&active, &cfg).unwrap();
        assert_eq!(drack, Drack::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f35_36_link_roundtrip_ok() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 2);
            assert_eq!(primary.get_function(), 35);
            assert!(s2f36(&pass, &primary, Lrack::Ok).unwrap());
        });

        let cfg = DynamicEventReportConfig::new();
        let r = cfg.add_define_report(101, None, &[1]).unwrap();
        cfg.add_link_by_report(50, &[r]).unwrap();
        let lrack = s2f35_link(&active, &cfg).unwrap();
        assert_eq!(lrack, Lrack::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f37_38_enable_roundtrip_ok() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 2);
            assert_eq!(primary.get_function(), 37);
            assert!(s2f38(&pass, &primary, Erack::Ok).unwrap());
        });

        let cfg = DynamicEventReportConfig::new();
        cfg.add_enable_collection_event(Some("E1".into()), 50)
            .unwrap();
        let erack = s2f37_enable(&active, &cfg).unwrap();
        assert_eq!(erack, Erack::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s2f33_unexpected_function_errors() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            // Wrong function on purpose
            pass.send_data_reply(&primary, 2, 0, false, Secs2::empty())
                .unwrap();
        });

        let cfg = DynamicEventReportConfig::new();
        cfg.add_define_report(1, None, &[1]).unwrap();
        let err = s2f33_define(&active, &cfg).unwrap_err();
        match err {
            GemError::UnexpectedReply {
                expected_function: 34,
                got_function: Some(0),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
