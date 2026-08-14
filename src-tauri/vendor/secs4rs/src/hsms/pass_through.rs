//! HSMS message pass-through observers (try-send / sended / receive).
//!
//! Source: `AbstractHsmsMessagePassThroughObserverFacade` —
//! `PutToTrySend` / `PutToSended` / `PutToReceive`.
//! Idiomatic: sync listener list (no ExecutorService queue); invoke on caller thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::message::HsmsMessage;
use crate::property::ListenerId;

type Listener = Arc<dyn Fn(&HsmsMessage) + Send + Sync + 'static>;

/// Three-way HSMS message pass-through (`HsmsMessagePassThroughObservable`).
#[derive(Default)]
pub struct HsmsPassThrough {
    try_send: Mutex<Vec<(ListenerId, Listener)>>,
    sended: Mutex<Vec<(ListenerId, Listener)>>,
    receive: Mutex<Vec<(ListenerId, Listener)>>,
    next_id: AtomicU64,
}

impl HsmsPassThrough {
    pub fn new() -> Self {
        Self {
            try_send: Mutex::new(Vec::new()),
            sended: Mutex::new(Vec::new()),
            receive: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> ListenerId {
        ListenerId::from_raw(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    fn add(
        list: &Mutex<Vec<(ListenerId, Listener)>>,
        id: ListenerId,
        f: Listener,
    ) -> bool {
        list.lock().expect("pass-through listeners").push((id, f));
        true
    }

    fn remove(list: &Mutex<Vec<(ListenerId, Listener)>>, id: ListenerId) -> bool {
        let mut g = list.lock().expect("pass-through listeners");
        let before = g.len();
        g.retain(|(i, _)| *i != id);
        g.len() < before
    }

    fn fire(list: &Mutex<Vec<(ListenerId, Listener)>>, msg: &HsmsMessage) {
        let snap: Vec<Listener> = list
            .lock()
            .expect("pass-through listeners")
            .iter()
            .map(|(_, l)| Arc::clone(l))
            .collect();
        for l in snap {
            l(msg);
        }
    }

    /// `AddTrySendHsmsMessagePassThroughListener`.
    pub fn add_try_send<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        let id = self.next_id();
        Self::add(&self.try_send, id, Arc::new(f));
        id
    }

    /// `RemoveTrySendHsmsMessagePassThroughListener`.
    pub fn remove_try_send(&self, id: ListenerId) -> bool {
        Self::remove(&self.try_send, id)
    }

    /// `AddSendedHsmsMessagePassThroughListener`.
    pub fn add_sended<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        let id = self.next_id();
        Self::add(&self.sended, id, Arc::new(f));
        id
    }

    pub fn remove_sended(&self, id: ListenerId) -> bool {
        Self::remove(&self.sended, id)
    }

    /// `AddReceiveHsmsMessagePassThroughListener`.
    pub fn add_receive<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        let id = self.next_id();
        Self::add(&self.receive, id, Arc::new(f));
        id
    }

    pub fn remove_receive(&self, id: ListenerId) -> bool {
        Self::remove(&self.receive, id)
    }

    /// `PutToTrySendHsmsMessage`.
    pub fn put_try_send(&self, msg: &HsmsMessage) {
        Self::fire(&self.try_send, msg);
    }

    /// `PutToSendedHsmsMessage`.
    pub fn put_sended(&self, msg: &HsmsMessage) {
        Self::fire(&self.sended, msg);
    }

    /// `PutToReceiveHsmsMessage`.
    pub fn put_receive(&self, msg: &HsmsMessage) {
        Self::fire(&self.receive, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn data_msg() -> HsmsMessage {
        HsmsMessage::of(&[0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]).unwrap()
    }

    #[test]
    fn pass_through_try_sended_receive() {
        let pt = HsmsPassThrough::new();
        let try_n = Arc::new(AtomicUsize::new(0));
        let sent_n = Arc::new(AtomicUsize::new(0));
        let recv_n = Arc::new(AtomicUsize::new(0));
        let a = Arc::clone(&try_n);
        let b = Arc::clone(&sent_n);
        let c = Arc::clone(&recv_n);
        let id_try = pt.add_try_send(move |_| {
            a.fetch_add(1, Ordering::SeqCst);
        });
        let id_s = pt.add_sended(move |_| {
            b.fetch_add(1, Ordering::SeqCst);
        });
        let id_r = pt.add_receive(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });

        let m = data_msg();
        pt.put_try_send(&m);
        pt.put_sended(&m);
        pt.put_receive(&m);
        assert_eq!(try_n.load(Ordering::SeqCst), 1);
        assert_eq!(sent_n.load(Ordering::SeqCst), 1);
        assert_eq!(recv_n.load(Ordering::SeqCst), 1);

        assert!(pt.remove_try_send(id_try));
        assert!(pt.remove_sended(id_s));
        assert!(pt.remove_receive(id_r));
        pt.put_try_send(&m);
        assert_eq!(try_n.load(Ordering::SeqCst), 1);
    }
}
