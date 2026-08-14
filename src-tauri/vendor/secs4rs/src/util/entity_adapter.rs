//! Entity event adapter: SxFy dispatch + automatic SxF0 / S9Fy routing.
//!
//! Source: `AbstractEntityEventAdapter.Received` / listener registration.
//! Communicator I/O is injected via [`EntityReplySink`] (idiomatic; no inheritance).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::secs2;
use crate::SecsMessage;

/// Side-effects the adapter needs when auto-replying (send / S9Fx).
pub trait EntityReplySink {
    fn device_id(&self) -> i32;
    fn session_id(&self) -> i32;

    /// `communicator.Send(primary, strm, func, wbit)` — header reply path.
    fn send_reply(&mut self, primary: &dyn SecsMessage, strm: i32, func: i32, wbit: bool);

    /// GEM S9F1 Unrecognized Device ID.
    fn s9f1(&mut self, msg: &dyn SecsMessage);
    /// GEM S9F3 Unrecognized Stream.
    fn s9f3(&mut self, msg: &dyn SecsMessage);
    /// GEM S9F5 Unrecognized Function.
    fn s9f5(&mut self, msg: &dyn SecsMessage);
    /// GEM S9F7 Illegal Data (listener hit Secs2 error).
    fn s9f7(&mut self, msg: &dyn SecsMessage);
}

/// Message receive listener (`EntityMessageReceiveListener`).
///
/// Return `Err(secs2::Error)` to trigger S9F7 (parity with `Secs2Exception`).
pub type EntityMessageListener =
    Arc<dyn Fn(&dyn SecsMessage) -> Result<(), secs2::Error> + Send + Sync + 'static>;

/// Communicatable state listener (`EntityCommunicatableStateChangeListener`).
pub type EntityStateListener = Arc<dyn Fn(bool) + Send + Sync + 'static>;

/// Entity event adapter (`EntityEventAdapter`).
pub struct EntityEventAdapter {
    msg_listeners: Mutex<HashMap<(i32, i32), EntityMessageListener>>,
    state_listeners: Mutex<Vec<EntityStateListener>>,
    reply_sxf0: AtomicBool,
    reply_s9fy: AtomicBool,
}

impl Default for EntityEventAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityEventAdapter {
    /// `EntityEventAdapter.NewInstance()`.
    pub fn new() -> Self {
        Self {
            msg_listeners: Mutex::new(HashMap::new()),
            state_listeners: Mutex::new(Vec::new()),
            reply_sxf0: AtomicBool::new(false),
            reply_s9fy: AtomicBool::new(false),
        }
    }

    /// `SetReplySxF0`.
    pub fn set_reply_sxf0(&self, do_reply: bool) {
        self.reply_sxf0.store(do_reply, Ordering::SeqCst);
    }

    /// `SetReplyS9Fy`.
    pub fn set_reply_s9fy(&self, do_reply: bool) {
        self.reply_s9fy.store(do_reply, Ordering::SeqCst);
    }

    pub fn is_reply_sxf0(&self) -> bool {
        self.reply_sxf0.load(Ordering::SeqCst)
    }

    pub fn is_reply_s9fy(&self) -> bool {
        self.reply_s9fy.load(Ordering::SeqCst)
    }

    /// `AddMessageReceiveListener` — keyed by stream/function only (one slot per SxFy).
    ///
    /// Returns `true` if newly registered; `false` if SxFy already had a listener (HashSet parity).
    pub fn add_message_receive_listener<F>(&self, strm: i32, func: i32, listener: F) -> bool
    where
        F: Fn(&dyn SecsMessage) -> Result<(), secs2::Error> + Send + Sync + 'static,
    {
        let mut g = self.msg_listeners.lock().expect("msg listeners");
        use std::collections::hash_map::Entry;
        match g.entry((strm, func)) {
            Entry::Vacant(e) => {
                e.insert(Arc::new(listener));
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// `RemoveMessageReceiveListener`.
    pub fn remove_message_receive_listener(&self, strm: i32, func: i32) -> bool {
        self.msg_listeners
            .lock()
            .expect("msg listeners")
            .remove(&(strm, func))
            .is_some()
    }

    /// `AddCommunicatableStateChangeListener` — always returns true (COW list parity).
    pub fn add_communicatable_state_change_listener<F>(&self, listener: F) -> bool
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.state_listeners
            .lock()
            .expect("state listeners")
            .push(Arc::new(listener));
        true
    }

    /// Notify communicatable state change (`Changed`).
    pub fn changed(&self, communicatable: bool) {
        let snap: Vec<EntityStateListener> = self
            .state_listeners
            .lock()
            .expect("state listeners")
            .clone();
        for l in snap {
            l(communicatable);
        }
    }

    /// `Received` — dispatch / auto SxF0 / S9Fy per protocol rules.
    pub fn received(&self, message: &dyn SecsMessage, sink: &mut dyn EntityReplySink) {
        if equals_device_id_or_session_id(message, sink.device_id(), sink.session_id()) {
            let strm = message.get_stream();
            let func = message.get_function();

            let (matched, stream_known) = {
                let g = self.msg_listeners.lock().expect("msg listeners");
                let matched = g.get(&(strm, func)).cloned();
                let stream_known = g.keys().any(|(s, _)| *s == strm);
                (matched, stream_known)
            };

            if let Some(listener) = matched {
                match listener(message) {
                    Ok(()) => {}
                    Err(_secs2) => {
                        // S9F7: illegal data while listener processed the message
                        // (gated like other auto S9Fy replies).
                        if self.is_reply_s9fy() {
                            sink.s9f7(message);
                        }
                    }
                }
            } else {
                let wbit = message.wbit();
                let sxf0 = self.is_reply_sxf0();
                let s9 = self.is_reply_s9fy();

                if stream_known {
                    // Unrecognized Function → optional SxF0 + S9F5
                    if wbit && sxf0 {
                        sink.send_reply(message, strm, 0, false);
                    }
                    if s9 {
                        sink.s9f5(message);
                    }
                } else {
                    // Unrecognized Stream → optional S0F0 + S9F3
                    if wbit && sxf0 {
                        sink.send_reply(message, 0, 0, false);
                    }
                    if s9 {
                        sink.s9f3(message);
                    }
                }
            }
        } else if self.is_reply_s9fy() {
            // S9F1: unrecognized Device-ID
            sink.s9f1(message);
        }
    }
}

/// Device/session id match (`EqualsDeviceIdOrSessionId`).
///
/// Negative ids fall back to session; if either side remains negative → false.
pub fn equals_device_id_or_session_id(
    message: &dyn SecsMessage,
    comm_device_id: i32,
    comm_session_id: i32,
) -> bool {
    let mut msg_id = message.device_id();
    let mut comm_id = comm_device_id;
    if msg_id < 0 {
        msg_id = message.session_id();
    }
    if comm_id < 0 {
        comm_id = comm_session_id;
    }
    if msg_id < 0 || comm_id < 0 {
        return false;
    }
    msg_id == comm_id
}

/// SECS-II body for S9Fx: Binary(MHEAD) = ref message header-10-bytes.
///
/// Source: `AbstractGem.S9fx` → `Secs2.Binary(refMsg.Header10Bytes())`.
pub fn s9_mhead_body(ref_header10: &[u8; 10]) -> secs2::Result<crate::secs2::Secs2> {
    crate::secs2::Secs2::binary(ref_header10.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::HsmsMessage;
    use crate::secs2::Secs2;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Rec {
        device: i32,
        session: i32,
        replies: Vec<(i32, i32, bool)>,
        s9: Vec<i32>,
    }

    impl EntityReplySink for Rec {
        fn device_id(&self) -> i32 {
            self.device
        }
        fn session_id(&self) -> i32 {
            self.session
        }
        fn send_reply(&mut self, _p: &dyn SecsMessage, strm: i32, func: i32, wbit: bool) {
            self.replies.push((strm, func, wbit));
        }
        fn s9f1(&mut self, _: &dyn SecsMessage) {
            self.s9.push(1);
        }
        fn s9f3(&mut self, _: &dyn SecsMessage) {
            self.s9.push(3);
        }
        fn s9f5(&mut self, _: &dyn SecsMessage) {
            self.s9.push(5);
        }
        fn s9f7(&mut self, _: &dyn SecsMessage) {
            self.s9.push(7);
        }
    }

    /// DATA header: session=10, stream/func/wbit, sys=AA BB CC DD
    fn data_msg(session: i32, stream: u8, func: u8, wbit: bool) -> HsmsMessage {
        let mut h = [0u8; 10];
        h[0] = (session >> 8) as u8;
        h[1] = session as u8;
        h[2] = stream | if wbit { 0x80 } else { 0 };
        h[3] = func;
        h[4] = 0; // p-type DATA
        h[5] = 0; // s-type DATA
        h[6] = 0xAA;
        h[7] = 0xBB;
        h[8] = 0xCC;
        h[9] = 0xDD;
        HsmsMessage::of_with_body(&h, Secs2::empty()).unwrap()
    }

    #[test]
    fn entity_dispatch_matched_listener() {
        let ad = EntityEventAdapter::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        assert!(ad.add_message_receive_listener(1, 1, move |_| {
            h.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        // second register same SxFy → false
        assert!(!ad.add_message_receive_listener(1, 1, |_| Ok(())));

        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(10, 1, 1, true), &mut sink);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(sink.replies.is_empty());
        assert!(sink.s9.is_empty());
    }

    #[test]
    fn entity_unrecognized_function_sxf0_and_s9f5() {
        let ad = EntityEventAdapter::new();
        ad.add_message_receive_listener(1, 1, |_| Ok(())); // stream 1 known
        ad.set_reply_sxf0(true);
        ad.set_reply_s9fy(true);

        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        // S1F99 W — unknown function on known stream
        ad.received(&data_msg(10, 1, 99, true), &mut sink);
        assert_eq!(sink.replies, vec![(1, 0, false)]); // S1F0
        assert_eq!(sink.s9, vec![5]);
    }

    #[test]
    fn entity_unrecognized_stream_s0f0_and_s9f3() {
        let ad = EntityEventAdapter::new();
        ad.add_message_receive_listener(1, 1, |_| Ok(()));
        ad.set_reply_sxf0(true);
        ad.set_reply_s9fy(true);

        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(10, 5, 1, true), &mut sink);
        assert_eq!(sink.replies, vec![(0, 0, false)]); // S0F0
        assert_eq!(sink.s9, vec![3]);
    }

    #[test]
    fn entity_wrong_device_s9f1() {
        let ad = EntityEventAdapter::new();
        ad.set_reply_s9fy(true);
        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(20, 1, 1, true), &mut sink);
        assert_eq!(sink.s9, vec![1]);
        assert!(sink.replies.is_empty());
    }

    #[test]
    fn entity_flags_off_no_auto_reply() {
        let ad = EntityEventAdapter::new();
        ad.add_message_receive_listener(1, 1, |_| Ok(()));
        // flags default false
        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(10, 1, 99, true), &mut sink);
        assert!(sink.replies.is_empty());
        assert!(sink.s9.is_empty());
    }

    #[test]
    fn entity_listener_secs2_error_s9f7() {
        let ad = EntityEventAdapter::new();
        ad.set_reply_s9fy(true);
        ad.add_message_receive_listener(1, 1, |_| Err(secs2::Error::IllegalDataFormat("x")));
        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(10, 1, 1, true), &mut sink);
        assert_eq!(sink.s9, vec![7]);
    }

    #[test]
    fn entity_listener_secs2_error_no_s9f7_when_disabled() {
        let ad = EntityEventAdapter::new();
        ad.set_reply_s9fy(false);
        ad.add_message_receive_listener(1, 1, |_| Err(secs2::Error::IllegalDataFormat("x")));
        let mut sink = Rec {
            device: 10,
            session: 10,
            ..Default::default()
        };
        ad.received(&data_msg(10, 1, 1, true), &mut sink);
        assert!(sink.s9.is_empty());
    }

    #[test]
    fn entity_s9_mhead_body() {
        let h = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let body = s9_mhead_body(&h).unwrap();
        for (i, b) in h.iter().enumerate() {
            assert_eq!(body.get_byte_at(&[i]).unwrap(), *b);
        }
        assert_eq!(body.size(), 10);
    }

    #[test]
    fn entity_device_id_fallback_session() {
        // message device_id for HSMS is session; test pure helper with a mock-ish HSMS msg
        let m = data_msg(10, 1, 1, false);
        assert!(equals_device_id_or_session_id(&m, 10, 99));
        assert!(!equals_device_id_or_session_id(&m, 11, 10));
        // both negative → false
        assert!(!equals_device_id_or_session_id(&m, -1, -1));
    }

    #[test]
    fn entity_state_change_listener() {
        let ad = EntityEventAdapter::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        ad.add_communicatable_state_change_listener(move |c| {
            if c {
                h.fetch_add(1, Ordering::SeqCst);
            }
        });
        ad.changed(true);
        ad.changed(false);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
