//! SecsCommunicatorEntity facade for HSMS-SS.
//!
//! Source: `AbstractSecsCommunicatorEntity` — composition of
//! `EntityEventAdapter` + `EntityMessageSender` over a communicator.
//!
//! Auto wire (`AdaptToSecsCommunicator` bi-listeners) is explicit here:
//! call [`HsmsSsEntity::handle_received`] / [`HsmsSsEntity::serve_one`]
//! after `take_data_message` (idiomatic; no inheritance / bi-listener bus).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::hsms::{Error as HsmsError, HsmsMessage};
use crate::hsms_ss::HsmsSsCommunicator;
use crate::secs2::{self, Secs2};
use crate::SecsMessage;

use super::entity_adapter::EntityEventAdapter;
use super::entity_sender::EntityMessageSender;
use super::entity_ss::HsmsSsEntitySink;

/// HSMS-SS entity (`SecsCommunicatorEntity.NewInstance`).
///
/// Aggregates SxFy routing + T3→S9F9 send over one [`HsmsSsCommunicator`].
pub struct HsmsSsEntity {
    comm: Arc<HsmsSsCommunicator>,
    adapter: EntityEventAdapter,
    /// Outbound S9F9-on-T3 flag (paired with adapter `reply_s9fy` via [`Self::set_send_s9fy`]).
    send_s9f9: AtomicBool,
}

impl HsmsSsEntity {
    /// `SecsCommunicatorEntity.NewInstance(communicator)`.
    pub fn new(comm: Arc<HsmsSsCommunicator>) -> Self {
        Self {
            comm,
            adapter: EntityEventAdapter::new(),
            send_s9f9: AtomicBool::new(false),
        }
    }

    /// Underlying HSMS-SS communicator.
    pub fn communicator(&self) -> &HsmsSsCommunicator {
        &self.comm
    }

    /// Shared communicator handle.
    pub fn communicator_arc(&self) -> Arc<HsmsSsCommunicator> {
        Arc::clone(&self.comm)
    }

    /// Event adapter (SxFy dispatch / auto SxF0 / S9Fy).
    pub fn adapter(&self) -> &EntityEventAdapter {
        &self.adapter
    }

    /// `IsOpen`.
    pub fn is_open(&self) -> bool {
        self.comm.is_open()
    }

    /// `IsClosed`.
    pub fn is_closed(&self) -> bool {
        self.comm.is_closed()
    }

    /// `Dispose` / close underlying communicator.
    pub fn close(&self) {
        self.comm.close();
    }

    /// Open Active HSMS-SS session.
    pub fn open_active(&self) -> Result<(), HsmsError> {
        self.comm.open_active()
    }

    /// Open Passive HSMS-SS session (blocking accept + select).
    pub fn open_passive(&self) -> Result<(), HsmsError> {
        self.comm.open_passive()
    }

    /// `SetReplySxF0`.
    pub fn set_reply_sxf0(&self, do_reply: bool) {
        self.adapter.set_reply_sxf0(do_reply);
    }

    /// `SetSendS9Fy` — inbound auto S9Fy **and** outbound T3→S9F9.
    pub fn set_send_s9fy(&self, do_send: bool) {
        self.adapter.set_reply_s9fy(do_send);
        self.send_s9f9.store(do_send, Ordering::SeqCst);
    }

    /// Whether S9Fy auto path is enabled (inbound + outbound share the entity flag).
    pub fn is_send_s9fy(&self) -> bool {
        self.adapter.is_reply_s9fy() && self.send_s9f9.load(Ordering::SeqCst)
    }

    /// `AddMessageReceiveListener` (one slot per SxFy).
    pub fn add_message_receive_listener<F>(&self, strm: i32, func: i32, listener: F) -> bool
    where
        F: Fn(&dyn SecsMessage) -> Result<(), secs2::Error> + Send + Sync + 'static,
    {
        self.adapter.add_message_receive_listener(strm, func, listener)
    }

    /// `RemoveMessageReceiveListener`.
    pub fn remove_message_receive_listener(&self, strm: i32, func: i32) -> bool {
        self.adapter.remove_message_receive_listener(strm, func)
    }

    /// `AddCommunicatableStateChangeListener`.
    pub fn add_communicatable_state_change_listener<F>(&self, listener: F) -> bool
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.adapter
            .add_communicatable_state_change_listener(listener)
    }

    /// Notify state listeners (`Changed`).
    pub fn changed(&self, communicatable: bool) {
        self.adapter.changed(communicatable);
    }

    fn sender(&self) -> EntityMessageSender<'_> {
        let s = EntityMessageSender::new(&self.comm);
        s.set_send_s9f9(self.send_s9f9.load(Ordering::SeqCst));
        s
    }

    /// `Send(strm, func, wbit, secs2)` with T3→S9F9 when flag on.
    pub fn send_data(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.sender().send_data(strm, func, wbit, body)
    }

    /// `Send(strm, func, wbit)` header-only primary.
    pub fn send_header(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.sender().send_header(strm, func, wbit)
    }

    /// `Send(primary, strm, func, wbit, secs2)` reply path.
    pub fn send_data_reply(
        &self,
        primary: &HsmsMessage,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<(), HsmsError> {
        self.sender()
            .send_data_reply(primary, strm, func, wbit, body)
    }

    /// `Send(SmlMessage)`.
    pub fn send_sml(
        &self,
        sml: &crate::sml::SmlMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.sender().send_sml(sml)
    }

    /// `SendS9F9` — only when S9Fy flag enabled.
    pub fn send_s9f9(
        &self,
        primary: &dyn SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.sender().send_s9f9(primary)
    }

    /// Route one primary through the adapter (`Received` + SS sink).
    ///
    /// Returns last sink I/O error if any (routing continues after individual failures).
    pub fn handle_received(&self, msg: &dyn SecsMessage) -> Option<HsmsError> {
        let mut sink = HsmsSsEntitySink::new(&self.comm);
        sink.received(&self.adapter, msg);
        sink.last_error
    }

    /// Take one DATA primary and route it (`take_data_message` + `handle_received`).
    ///
    /// Returns `(message, sink_error)`.
    pub fn serve_one(&self) -> Result<(HsmsMessage, Option<HsmsError>), HsmsError> {
        let msg = self.comm.take_data_message()?;
        let err = self.handle_received(&msg);
        Ok((msg, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::{HsmsCommunicateState, HsmsConnectionMode};
    use crate::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
    use crate::SecsMessage as _;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
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
    fn entity_facade_flags_and_listener() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let cfg = HsmsSsCommunicatorConfig::new();
        cfg.set_session_id(1).unwrap();
        cfg.set_connection_mode(HsmsConnectionMode::Passive);
        cfg.set_socket_address(addr);
        let comm = Arc::new(HsmsSsCommunicator::new_instance(cfg));
        let entity = HsmsSsEntity::new(comm);

        assert!(!entity.is_send_s9fy());
        entity.set_send_s9fy(true);
        assert!(entity.is_send_s9fy());
        assert!(entity.adapter().is_reply_s9fy());

        entity.set_reply_sxf0(true);
        assert!(entity.adapter().is_reply_sxf0());

        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        assert!(entity.add_message_receive_listener(1, 1, move |_| {
            h.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        assert!(!entity.add_message_receive_listener(1, 1, |_| Ok(())));
        assert!(entity.remove_message_receive_listener(1, 1));
        assert!(!entity.remove_message_receive_listener(1, 1));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn entity_facade_unrecognized_function_s1f0_and_s9f5() {
        let (active, passive) = open_pair();
        let entity = Arc::new(HsmsSsEntity::new(Arc::clone(&passive)));
        entity.add_message_receive_listener(1, 1, |_| Ok(())); // stream 1 known
        entity.set_reply_sxf0(true);
        entity.set_send_s9fy(true);

        let ent = Arc::clone(&entity);
        let worker = thread::spawn(move || {
            let (primary, err) = ent.serve_one().unwrap();
            assert_eq!(primary.get_stream(), 1);
            assert_eq!(primary.get_function(), 99);
            assert!(err.is_none(), "sink err={err:?}");
        });

        let reply = active
            .send_data(1, 99, true, Secs2::empty())
            .unwrap()
            .expect("S1F0 reply");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 0);
        assert!(!reply.wbit());

        let s9 = active
            .poll_data_message(Duration::from_secs(2))
            .unwrap()
            .expect("S9F5");
        assert_eq!(s9.get_stream(), 9);
        assert_eq!(s9.get_function(), 5);
        assert!(!s9.wbit());
        assert_eq!(s9.secs2().get_byte_at(&[2]).unwrap() & 0x7F, 1);
        assert_eq!(s9.secs2().get_byte_at(&[3]).unwrap(), 99);

        worker.join().unwrap();
        entity.close();
        active.close();
    }

    #[test]
    fn entity_facade_matched_listener_and_send() {
        let (active, passive) = open_pair();
        let entity = Arc::new(HsmsSsEntity::new(Arc::clone(&passive)));
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        entity.add_message_receive_listener(1, 13, move |_| {
            h.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });

        let ent = Arc::clone(&entity);
        let worker = thread::spawn(move || {
            let (primary, err) = ent.serve_one().unwrap();
            assert!(err.is_none());
            assert_eq!(primary.get_stream(), 1);
            assert_eq!(primary.get_function(), 13);
            assert!(primary.wbit());
            // Reply S1F14 COMMACK=0 via entity send path
            let body = Secs2::list([
                Secs2::binary([0]).unwrap(),
                Secs2::list([]).unwrap(),
            ])
            .unwrap();
            ent.send_data_reply(&primary, 1, 14, false, body)
                .unwrap();
        });

        let host = HsmsSsEntity::new(Arc::clone(&active));
        let reply = host
            .send_data(1, 13, true, Secs2::list([]).unwrap())
            .unwrap()
            .expect("S1F14");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 14);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        worker.join().unwrap();
        entity.close();
        active.close();
    }
}
