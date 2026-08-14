//! GEM Stream-7 process-program ACK helpers.
//!
//! Source: `AbstractGem.S7f*` via shared `S7fx` (ACKC7 reply).

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::SecsMessage;

use super::ack::Ackc7;
use super::error::GemError;

fn s7fx(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    func: i32,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 7, func, false, ackc7.secs2())?;
    Ok(true)
}

/// S7F4 Process Program Acknowledge.
pub fn s7f4(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 4, ackc7)
}

/// S7F12 Matrix Acknowledge.
pub fn s7f12(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 12, ackc7)
}

/// S7F14 Delete Process Program Acknowledge.
pub fn s7f14(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 14, ackc7)
}

/// S7F16 Matrix Mode Acknowledge.
pub fn s7f16(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 16, ackc7)
}

/// S7F18 Delete Acknowledge.
pub fn s7f18(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 18, ackc7)
}

/// S7F24 Acknowledge.
pub fn s7f24(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 24, ackc7)
}

/// S7F32 Acknowledge.
pub fn s7f32(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 32, ackc7)
}

/// S7F38 Acknowledge.
pub fn s7f38(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 38, ackc7)
}

/// S7F40 Acknowledge.
pub fn s7f40(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 40, ackc7)
}

/// S7F42 Acknowledge.
pub fn s7f42(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 42, ackc7)
}

/// S7F44 Acknowledge.
pub fn s7f44(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc7: Ackc7,
) -> Result<bool, GemError> {
    s7fx(comm, primary, 44, ackc7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::{HsmsCommunicateState, HsmsConnectionMode};
    use crate::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
    use crate::secs2::Secs2;
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
    fn s7f4_ackc7_roundtrip() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 7);
            assert_eq!(primary.get_function(), 3);
            assert!(s7f4(&pass, &primary, Ackc7::Accepted).unwrap());
        });

        // Host sends S7F3 W (process program request shape not fully modeled)
        let reply = active
            .send_data(7, 3, true, Secs2::ascii("PPID").unwrap())
            .unwrap()
            .expect("S7F4");
        assert_eq!(reply.get_stream(), 7);
        assert_eq!(reply.get_function(), 4);
        assert_eq!(Ackc7::from_secs2(reply.secs2()).unwrap(), Ackc7::Accepted);

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn s7f12_permission_not_granted() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert!(s7f12(&pass, &primary, Ackc7::PermissionNotGranted).unwrap());
        });

        let reply = active
            .send_data(7, 11, true, Secs2::empty())
            .unwrap()
            .expect("S7F12");
        assert_eq!(
            Ackc7::from_secs2(reply.secs2()).unwrap(),
            Ackc7::PermissionNotGranted
        );

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
