//! GEM Stream-10 terminal-service ACK helpers.
//!
//! Source: `AbstractGem.S10fx` — **non-reply** primary send of S10Fy when W-bit set
//! (Java/C# parity: `Send(10, func, false, ACKC10)`, not `Send(primary, …)`).

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::SecsMessage;

use super::ack::Ackc10;
use super::error::GemError;

/// Shared Stream-10 ACK path (`S10fx`).
///
/// Returns `Ok(false)` if primary has no W-bit (source returns null).
/// On success sends a **new primary** S10Fy (not a transaction reply) → `Ok(true)`.
fn s10fx(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    func: i32,
    ackc10: Ackc10,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    // Parity: primary send, NOT data-reply (new system-bytes).
    comm.send_data(10, func, false, ackc10.secs2())?;
    Ok(true)
}

/// S10F2 Terminal Request Acknowledge.
pub fn s10f2(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc10: Ackc10,
) -> Result<bool, GemError> {
    s10fx(comm, primary, 2, ackc10)
}

/// S10F4 Terminal Display Single Acknowledge.
pub fn s10f4(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc10: Ackc10,
) -> Result<bool, GemError> {
    s10fx(comm, primary, 4, ackc10)
}

/// S10F6 Terminal Display Multi Acknowledge.
pub fn s10f6(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc10: Ackc10,
) -> Result<bool, GemError> {
    s10fx(comm, primary, 6, ackc10)
}

/// S10F10 Broadcast Acknowledge.
pub fn s10f10(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    ackc10: Ackc10,
) -> Result<bool, GemError> {
    s10fx(comm, primary, 10, ackc10)
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
        a_cfg.timeout().set_t3(0.3); // short T3 for S10 non-reply parity tests
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
    fn s10f2_is_primary_not_transaction_reply() {
        // S10fx uses Send(10,f,false) — host's W-bit S10F1 transaction gets no reply;
        // ACK arrives as a separate primary S10F2 with new system-bytes.
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 10);
            assert_eq!(primary.get_function(), 1);
            assert!(primary.wbit());
            assert!(s10f2(&pass, &primary, Ackc10::AcceptedForDisplay).unwrap());
            primary
        });

        let act = Arc::clone(&active);
        let ack_collector = thread::spawn(move || {
            for _ in 0..40 {
                if let Ok(Some(m)) = act.poll_data_message(Duration::from_millis(50)) {
                    if m.get_stream() == 10 && m.get_function() == 2 {
                        return m;
                    }
                }
            }
            panic!("S10F2 primary not received");
        });

        let err = active
            .send_data(10, 1, true, Secs2::ascii("TID").unwrap())
            .unwrap_err();
        assert!(matches!(err, crate::hsms::Error::TimeoutT3 { .. }));

        let s10f2_msg = ack_collector.join().unwrap();
        assert_eq!(s10f2_msg.get_stream(), 10);
        assert_eq!(s10f2_msg.get_function(), 2);
        assert!(!s10f2_msg.wbit());
        assert_eq!(
            Ackc10::from_secs2(s10f2_msg.secs2()).unwrap(),
            Ackc10::AcceptedForDisplay
        );

        let primary = worker.join().unwrap();
        assert_ne!(
            s10f2_msg.header10_bytes()[6..10],
            primary.header10_bytes()[6..10],
            "S10F2 must be a new primary (different system-bytes), not a reply"
        );

        active.close();
        passive.close();
    }

    #[test]
    fn s10f2_no_wbit_returns_false() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            assert!(!primary.wbit());
            assert!(!s10f2(&pass, &primary, Ackc10::AcceptedForDisplay).unwrap());
        });

        let reply = active.send_data(10, 1, false, Secs2::empty()).unwrap();
        assert!(reply.is_none());

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
