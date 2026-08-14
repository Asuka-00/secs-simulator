//! Bind [`EntityEventAdapter`] to HSMS-SS communicator.
//!
//! Source: entity routing over `SecsCommunicator` send/GEM S9 paths.

use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::Secs2;
use crate::SecsMessage;

use super::entity_adapter::{EntityEventAdapter, EntityReplySink};

/// `EntityReplySink` backed by [`HsmsSsCommunicator`].
///
/// Auto SxF0 → `send_data_reply_from_header`; S9Fx → `send_s9f*`.
pub struct HsmsSsEntitySink<'a> {
    comm: &'a HsmsSsCommunicator,
    /// Last I/O error (if any); does not stop remaining routing side-effects.
    pub last_error: Option<crate::hsms::Error>,
}

impl<'a> HsmsSsEntitySink<'a> {
    pub fn new(comm: &'a HsmsSsCommunicator) -> Self {
        Self {
            comm,
            last_error: None,
        }
    }

    fn record_err(&mut self, r: Result<(), crate::hsms::Error>) {
        if let Err(e) = r {
            self.last_error = Some(e);
        }
    }

    /// Dispatch one primary through the adapter using this sink.
    pub fn received(&mut self, adapter: &EntityEventAdapter, msg: &dyn SecsMessage) {
        adapter.received(msg, self);
    }
}

impl EntityReplySink for HsmsSsEntitySink<'_> {
    fn device_id(&self) -> i32 {
        self.comm.session_id()
    }

    fn session_id(&self) -> i32 {
        self.comm.session_id()
    }

    fn send_reply(&mut self, primary: &dyn SecsMessage, strm: i32, func: i32, wbit: bool) {
        let h = primary.header10_bytes();
        let r = self
            .comm
            .send_data_reply_from_header(&h, strm, func, wbit, Secs2::empty())
            .map(|_| ());
        self.record_err(r);
    }

    fn s9f1(&mut self, msg: &dyn SecsMessage) {
        let r = self.comm.send_s9f1(msg).map(|_| ());
        self.record_err(r);
    }

    fn s9f3(&mut self, msg: &dyn SecsMessage) {
        let r = self.comm.send_s9f3(msg).map(|_| ());
        self.record_err(r);
    }

    fn s9f5(&mut self, msg: &dyn SecsMessage) {
        let r = self.comm.send_s9f5(msg).map(|_| ());
        self.record_err(r);
    }

    fn s9f7(&mut self, msg: &dyn SecsMessage) {
        let r = self.comm.send_s9f7(msg).map(|_| ());
        self.record_err(r);
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

    #[test]
    fn entity_ss_unrecognized_function_s1f0_and_s9f5() {
        // Passive knows S1F1 only; Active sends S1F99 W → S1F0 reply + S9F5 primary.
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

        let adapter = EntityEventAdapter::new();
        adapter.add_message_receive_listener(1, 1, |_| Ok(())); // stream 1 known
        adapter.set_reply_sxf0(true);
        adapter.set_reply_s9fy(true);

        // Passive worker: take primary → entity route
        let pass = Arc::clone(&passive);
        let worker = thread::spawn(move || {
            let primary = pass.take_data_message().unwrap();
            let mut sink = HsmsSsEntitySink::new(&pass);
            sink.received(&adapter, &primary);
            assert!(sink.last_error.is_none(), "sink err={:?}", sink.last_error);
        });

        // Active: S1F99 W — expect S1F0 as transaction reply
        let reply = active
            .send_data(1, 99, true, Secs2::empty())
            .unwrap()
            .expect("S1F0 reply");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 0);
        assert!(!reply.wbit());

        // S9F5 arrives as primary on active
        let s9 = active
            .poll_data_message(Duration::from_secs(2))
            .unwrap()
            .expect("S9F5");
        assert_eq!(s9.get_stream(), 9);
        assert_eq!(s9.get_function(), 5);
        assert!(!s9.wbit());
        // MHEAD body matches original primary header (S1F99)
        // system-bytes of original request are in reply header; MHEAD is ref header
        for i in 0..4 {
            // first 4 bytes of MHEAD: session + stream/func
            let _ = s9.secs2().get_byte_at(&[i]).unwrap();
        }
        assert_eq!(s9.secs2().get_byte_at(&[2]).unwrap() & 0x7F, 1); // stream 1
        assert_eq!(s9.secs2().get_byte_at(&[3]).unwrap(), 99); // function 99

        worker.join().unwrap();
        active.close();
        passive.close();
    }
}
