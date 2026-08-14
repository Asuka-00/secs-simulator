//! GEM Stream-1 communication state helpers.
//!
//! Source: `AbstractGem.S1f1`…`S1f18`.

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::Secs2;
use crate::SecsMessage;

use super::ack::{CommAck, OflAck, OnlAck};
use super::config::GemConfig;
use super::error::{expect_reply, GemError};

/// S1F1 Are You There (header only, W-bit).
pub fn s1f1(comm: &HsmsSsCommunicator) -> Result<Option<HsmsMessage>, GemError> {
    Ok(comm.send_data(1, 1, true, Secs2::empty())?)
}

/// S1F2 Online Data reply with MDLN/SOFTREV.
pub fn s1f2(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    config: &GemConfig,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    let body = config.mdln_softrev()?;
    comm.send_data_reply(primary, 1, 2, false, body)?;
    Ok(true)
}

/// S1F13 Establish Communications; parse S1F14 → [`CommAck`].
pub fn s1f13(comm: &HsmsSsCommunicator, config: &GemConfig) -> Result<CommAck, GemError> {
    let body = config.mdln_softrev()?;
    let reply = comm.send_data(1, 13, true, body)?;
    let m = expect_reply(reply, 1, 14)?;
    // COMMACK is first list item: L <COMMACK> <L MDLN SOFTREV>
    let ack_item = m.secs2().get_item(&[0])?;
    Ok(CommAck::from_secs2(ack_item)?)
}

/// S1F14 reply: `L <COMMACK> <MDLN/SOFTREV>`.
pub fn s1f14(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    config: &GemConfig,
    commack: CommAck,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    let body = Secs2::list([commack.secs2(), config.mdln_softrev()?])?;
    comm.send_data_reply(primary, 1, 14, false, body)?;
    Ok(true)
}

/// S1F15 Request OFF-LINE → S1F16 OFLACK.
pub fn s1f15(comm: &HsmsSsCommunicator) -> Result<OflAck, GemError> {
    let reply = comm.send_data(1, 15, true, Secs2::empty())?;
    let m = expect_reply(reply, 1, 16)?;
    Ok(OflAck::from_secs2(m.secs2())?)
}

/// S1F16 OFF-LINE Acknowledge (always OFLACK.OK in source).
pub fn s1f16(comm: &HsmsSsCommunicator, primary: &HsmsMessage) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 1, 16, false, OflAck::Ok.secs2())?;
    Ok(true)
}

/// S1F17 Request ON-LINE → S1F18 ONLACK.
pub fn s1f17(comm: &HsmsSsCommunicator) -> Result<OnlAck, GemError> {
    let reply = comm.send_data(1, 17, true, Secs2::empty())?;
    let m = expect_reply(reply, 1, 18)?;
    Ok(OnlAck::from_secs2(m.secs2())?)
}

/// S1F18 ON-LINE Acknowledge.
pub fn s1f18(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    onlack: OnlAck,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 1, 18, false, onlack.secs2())?;
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
    fn s1f1_2_are_you_there() {
        let (active, passive) = open_pair();
        let pcfg = GemConfig::new();
        pcfg.set_is_equip(true);
        pcfg.set_mdln("EQ1");
        pcfg.set_softrev("1.0");

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 1);
            assert_eq!(primary.get_function(), 1);
            assert!(s1f2(&pass, &primary, &pcfg).unwrap());
        });

        let reply = s1f1(&active).unwrap().expect("S1F2");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 2);
        assert_eq!(reply.secs2().get_ascii_at(&[0]).unwrap(), "EQ1");
        assert_eq!(reply.secs2().get_ascii_at(&[1]).unwrap(), "1.0");

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s1f13_14_establish_comm() {
        let (active, passive) = open_pair();
        let acfg = GemConfig::new(); // host empty list
        let pcfg = GemConfig::new();
        pcfg.set_is_equip(true);
        pcfg.set_mdln("TOOL");
        pcfg.set_softrev("2.0");

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_function(), 13);
            assert_eq!(primary.secs2().size(), 0); // host empty
            assert!(s1f14(&pass, &primary, &pcfg, CommAck::Ok).unwrap());
        });

        let ack = s1f13(&active, &acfg).unwrap();
        assert_eq!(ack, CommAck::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s1f15_16_offline() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_function(), 15);
            assert!(s1f16(&pass, &primary).unwrap());
        });

        let ack = s1f15(&active).unwrap();
        assert_eq!(ack, OflAck::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s1f17_18_online() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_function(), 17);
            assert!(s1f18(&pass, &primary, OnlAck::Ok).unwrap());
        });

        let ack = s1f17(&active).unwrap();
        assert_eq!(ack, OnlAck::Ok);

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
