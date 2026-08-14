//! Entity message sender: optional S9F9 on T3 wait-reply timeout.
//!
//! Source: `AbstractEntityMessageSender` / `EntityMessageSender`.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::hsms::Error as HsmsError;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::Secs2;
use crate::SecsMessage;

/// Entity send wrapper (`EntityMessageSender`).
///
/// On W-bit T3 timeout, optionally sends S9F9 (Binary MHEAD of the unanswered primary)
/// then re-returns the original `TimeoutT3` error (parity: fire S9F9 then rethrow).
pub struct EntityMessageSender<'a> {
    comm: &'a HsmsSsCommunicator,
    do_send_s9f9: AtomicBool,
}

impl<'a> EntityMessageSender<'a> {
    /// `EntityMessageSender.NewInstance(communicator)`.
    pub fn new(comm: &'a HsmsSsCommunicator) -> Self {
        Self {
            comm,
            do_send_s9f9: AtomicBool::new(false),
        }
    }

    /// `SetSendS9F9`.
    pub fn set_send_s9f9(&self, do_send: bool) {
        self.do_send_s9f9.store(do_send, Ordering::SeqCst);
    }

    pub fn is_send_s9f9(&self) -> bool {
        self.do_send_s9f9.load(Ordering::SeqCst)
    }

    /// Underlying communicator.
    pub fn communicator(&self) -> &HsmsSsCommunicator {
        self.comm
    }

    /// `SendS9F9` — send only when flag enabled and `primary` is present.
    ///
    /// Returns `Ok(None)` when flag is off (C# returns null without send).
    pub fn send_s9f9(
        &self,
        primary: &dyn SecsMessage,
    ) -> Result<Option<crate::hsms::HsmsMessage>, HsmsError> {
        if !self.is_send_s9f9() {
            return Ok(None);
        }
        self.comm.send_s9f9(primary)
    }

    /// `SendS9F9IgnoreException` — swallow all HSMS errors.
    pub fn send_s9f9_ignore_exception(&self, primary: &dyn SecsMessage) {
        let _ = self.send_s9f9(primary);
    }

    /// After a wait-reply failure: optional S9F9 then re-return the error.
    fn after_wait_reply_err(&self, err: HsmsError) -> Result<Option<crate::hsms::HsmsMessage>, HsmsError> {
        if let HsmsError::TimeoutT3 { primary } = &err {
            self.send_s9f9_ignore_exception(primary);
        }
        Err(err)
    }

    /// `Send(strm, func, wbit, secs2)` with T3→S9F9.
    pub fn send_data(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<crate::hsms::HsmsMessage>, HsmsError> {
        match self.comm.send_data(strm, func, wbit, body) {
            Ok(r) => Ok(r),
            Err(e) => self.after_wait_reply_err(e),
        }
    }

    /// Header-only primary send (`Send(strm, func, wbit)`).
    pub fn send_header(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
    ) -> Result<Option<crate::hsms::HsmsMessage>, HsmsError> {
        self.send_data(strm, func, wbit, Secs2::empty())
    }

    /// Reply to primary (`Send(primary, strm, func, wbit, secs2)`).
    pub fn send_data_reply(
        &self,
        primary: &crate::hsms::HsmsMessage,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<(), HsmsError> {
        match self.comm.send_data_reply(primary, strm, func, wbit, body) {
            Ok(()) => Ok(()),
            Err(e) => {
                if let HsmsError::TimeoutT3 { primary } = &e {
                    self.send_s9f9_ignore_exception(primary);
                }
                Err(e)
            }
        }
    }

    /// `Send(SmlMessage)` — primary DATA from parsed SML, with T3→S9F9.
    pub fn send_sml(
        &self,
        sml: &crate::sml::SmlMessage,
    ) -> Result<Option<crate::hsms::HsmsMessage>, HsmsError> {
        self.send_data(
            sml.get_stream(),
            sml.get_function(),
            sml.wbit(),
            sml.secs2().clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::{HsmsCommunicateState, HsmsConnectionMode};
    use crate::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
    use crate::SecsMessage as _;
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
        a_cfg.timeout().set_t3(0.2); // 200ms T3
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
    fn entity_sender_t3_fires_s9f9() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            // Hold primary without reply so Active hits T3
            let primary = pass.take_data_message().unwrap();
            assert_eq!(primary.get_stream(), 1);
            assert_eq!(primary.get_function(), 1);
            assert!(primary.wbit());

            // Expect S9F9 from entity sender
            let s9 = pass
                .poll_data_message(Duration::from_secs(2))
                .unwrap()
                .expect("S9F9");
            assert_eq!(s9.get_stream(), 9);
            assert_eq!(s9.get_function(), 9);
            assert!(!s9.wbit());
            // MHEAD = unanswered S1F1 header
            assert_eq!(s9.secs2().get_byte_at(&[2]).unwrap() & 0x7F, 1);
            assert_eq!(s9.secs2().get_byte_at(&[3]).unwrap(), 1);
            s9
        });

        let sender = EntityMessageSender::new(&active);
        sender.set_send_s9f9(true);
        let err = sender
            .send_data(1, 1, true, Secs2::ascii("WAIT").unwrap())
            .unwrap_err();
        match err {
            HsmsError::TimeoutT3 { primary } => {
                assert_eq!(primary.get_stream(), 1);
                assert_eq!(primary.get_function(), 1);
                assert!(primary.wbit());
                assert_eq!(primary.secs2().get_ascii().unwrap(), "WAIT");
            }
            other => panic!("expected TimeoutT3, got {other:?}"),
        }

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn entity_sender_send_sml() {
        let (active, passive) = open_pair();
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let m = pass.take_data_message().unwrap();
            assert_eq!(m.get_stream(), 1);
            assert_eq!(m.get_function(), 13);
            assert!(!m.wbit());
            assert_eq!(m.secs2().get_ascii().unwrap(), "SML");
        });

        let sml = crate::sml::SmlMessage::of(r#"S1F13 <A "SML">."#).unwrap();
        let sender = EntityMessageSender::new(&active);
        let reply = sender.send_sml(&sml).unwrap();
        assert!(reply.is_none()); // no W-bit

        worker.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn entity_sender_t3_flag_off_no_s9f9() {
        let (active, passive) = open_pair();

        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let _ = pass.take_data_message().unwrap();
            // No S9F9 expected
            let s9 = pass.poll_data_message(Duration::from_millis(300)).unwrap();
            assert!(s9.is_none(), "S9F9 must not be sent when flag is off");
        });

        let sender = EntityMessageSender::new(&active);
        // flag default false
        let err = sender.send_header(1, 1, true).unwrap_err();
        assert!(matches!(err, HsmsError::TimeoutT3 { .. }));

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
