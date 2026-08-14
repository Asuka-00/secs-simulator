//! HSMS session I/O: reader thread, send queue, transaction map, primary queue.
//!
//! Source: `AbstractHsmsAsynchronousSocketChannelFacade` — send with reply
//! matching by system-bytes; unmatched receives go to primary queue.
//! Also: I/O activity → `LinktestReset` (defers periodic linktest).
//!
//! Idiomatic shape: dedicated sender thread (`TaskSendMessage` + `sendMsgQueue`);
//! `try_clone` stream for concurrent read; mpsc for send packs, reply slots,
//! and primary messages (no ExecutorService).

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::builder::{build_linktest_response, build_reject_request};
use super::channel::{read_frame_t8, write_frame};
use super::error::{Error, Result};
use super::message::HsmsMessage;
use super::message_type::HsmsMessageType;
use super::pass_through::HsmsPassThrough;
use super::status::RejectReason;

type ReplyTx = Sender<HsmsMessage>;

/// One outbound message + oneshot for “written to socket” (`WaitUntilSended`).
struct SendPack {
    msg: HsmsMessage,
    /// `Ok(())` after wire write; `Err` on I/O / shutdown / too-big / cancelled.
    done: Sender<Result<()>>,
    /// Set when WaitUntilSended times out so the sender must not write the frame.
    cancelled: Arc<AtomicBool>,
}

/// Idle-linktest activity gate (`LinktestReset` / `linktestResetFlag`).
///
/// Any send/recv touches the gate so the periodic linktest timer restarts
/// (SEMI: linktest only after idle for `LinktestTime`).
#[derive(Debug, Default)]
pub struct LinktestActivity {
    /// True when I/O happened since last clear.
    reset: Mutex<bool>,
    cv: Condvar,
}

impl LinktestActivity {
    pub fn new() -> Self {
        Self {
            reset: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    /// `LinktestReset` — mark recent I/O and wake waiters.
    pub fn touch(&self) {
        let mut g = self.reset.lock().expect("linktest activity");
        *g = true;
        self.cv.notify_all();
    }

    /// Wait until `interval` of continuous idle, or return early on activity / stop.
    ///
    /// Returns `true` when the idle interval completed without a touch (caller should
    /// issue linktest if enabled). Returns `false` if activity reset the timer or
    /// `stop` became true.
    pub fn wait_idle(&self, interval: Duration, stop: &AtomicBool) -> bool {
        let mut g = self.reset.lock().expect("linktest activity");
        *g = false;
        let deadline = Instant::now() + interval;
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            if *g {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                // Final check: activity during last slice?
                return !*g;
            }
            let rem = deadline.saturating_duration_since(now);
            let slice = rem.min(Duration::from_millis(20));
            let (guard, _) = self
                .cv
                .wait_timeout(g, slice)
                .expect("linktest activity wait");
            g = guard;
        }
    }
}

/// Connected HSMS channel with send queue + transaction map + primary queue.
///
/// `primary_rx` / `send_tx` are behind mutexes so `Arc<HsmsSessionIo>` is
/// `Send + Sync` (std `Receiver` alone is not `Sync`).
pub struct HsmsSessionIo {
    /// Clone used only for `shutdown` (Both); sender owns the write half.
    shutdown_stream: Mutex<TcpStream>,
    /// Enqueue path for `TaskSendMessage` (`sendMsgQueue.Put`).
    send_tx: Mutex<Option<Sender<SendPack>>>,
    transactions: Arc<Mutex<HashMap<i64, ReplyTx>>>,
    primary_rx: Mutex<Receiver<HsmsMessage>>,
    shutdown: Arc<AtomicBool>,
    reader: Mutex<Option<JoinHandle<()>>>,
    sender: Mutex<Option<JoinHandle<()>>>,
    /// Shared with periodic linktest task.
    activity: Arc<LinktestActivity>,
    /// Optional pass-through observers (try/sended/receive).
    pass_through: Arc<HsmsPassThrough>,
}

impl HsmsSessionIo {
    /// Take ownership of a connected stream; spawn receive + send loops.
    ///
    /// Reader uses SEMI T8 only **inside** a frame (not as whole-link idle timeout).
    pub fn new(stream: TcpStream) -> Result<Self> {
        Self::with_pass_through_t8(
            stream,
            Arc::new(HsmsPassThrough::new()),
            Duration::from_secs(6),
        )
    }

    /// Same as [`Self::new`] but shares an existing pass-through facade (communicator-owned).
    pub fn with_pass_through(stream: TcpStream, pass_through: Arc<HsmsPassThrough>) -> Result<Self> {
        Self::with_pass_through_t8(stream, pass_through, Duration::from_secs(6))
    }

    /// Like [`Self::with_pass_through`] with explicit T8 (inter-byte / incomplete-frame).
    pub fn with_pass_through_t8(
        stream: TcpStream,
        pass_through: Arc<HsmsPassThrough>,
        t8: Duration,
    ) -> Result<Self> {
        let writer = stream.try_clone().map_err(Error::from)?;
        let shutdown_stream = stream.try_clone().map_err(Error::from)?;
        let mut reader_stream = stream;
        // Do **not** set a permanent socket read timeout here.
        // T8 applies only after the first byte of a frame (`read_frame_t8`).
        // Idle Selected links must remain open without traffic (Linktest is optional).

        let transactions: Arc<Mutex<HashMap<i64, ReplyTx>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (primary_tx, primary_rx) = mpsc::channel();
        let (send_tx, send_rx) = mpsc::channel::<SendPack>();
        let shutdown = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(LinktestActivity::new());

        let tx_map = Arc::clone(&transactions);
        let shut = Arc::clone(&shutdown);
        let act = Arc::clone(&activity);
        let pt = Arc::clone(&pass_through);
        let reader = thread::spawn(move || {
            while !shut.load(Ordering::SeqCst) {
                match read_frame_t8(&mut reader_stream, t8) {
                    Ok(msg) => {
                        // Any receive counts as I/O activity (linktestReset).
                        act.touch();
                        // PutToReceiveHsmsMessage (all frames, before routing).
                        pt.put_receive(&msg);
                        let key = msg.system_bytes_key();
                        // Only response-class messages may complete a transaction.
                        // Incoming LINKTEST/SELECT *req* with the same system-bytes
                        // (common when both hosts use serial 1) must stay primary.
                        let as_reply = is_transaction_reply(&msg);
                        let mut map = tx_map.lock().expect("transaction map");
                        if as_reply {
                            if let Some(tx) = map.remove(&key) {
                                let _ = tx.send(msg);
                                continue;
                            }
                        }
                        drop(map);
                        if primary_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Err(Error::DetectTerminate) => break,
                    Err(Error::TimeoutT8) => {
                        // Incomplete frame after first byte — protocol abort.
                        break;
                    }
                    Err(e) if shut.load(Ordering::SeqCst) => {
                        let _ = e;
                        break;
                    }
                    Err(_e) => {
                        // Other I/O errors (reset, etc.) → tear down.
                        break;
                    }
                }
            }
            // Peer drop / T8 / local shutdown: drop reply channels so waiters
            // exit with Disconnected → DetectTerminate (no T3 hang).
            shut.store(true, Ordering::SeqCst);
            tx_map.lock().expect("transaction map").clear();
            // primary_tx drop ends take_primary waiters.
        });

        // TaskSendMessage: serialise writes; try-send → write → sended.
        let shut_s = Arc::clone(&shutdown);
        let act_s = Arc::clone(&activity);
        let pt_s = Arc::clone(&pass_through);
        let tx_map_s = Arc::clone(&transactions);
        let mut writer = writer;
        let sender = thread::spawn(move || {
            while let Ok(pack) = send_rx.recv() {
                if shut_s.load(Ordering::SeqCst) {
                    let _ = pack.done.send(Err(Error::ChannelShutdown));
                    // Drain remaining as shutdown (parity: HsmChannelAlreadyShutdown).
                    while let Ok(p) = send_rx.try_recv() {
                        let _ = p.done.send(Err(Error::ChannelShutdown));
                    }
                    break;
                }

                // WaitUntilSended timed out: do not write orphan frame.
                if pack.cancelled.load(Ordering::SeqCst) {
                    let _ = pack.done.send(Err(Error::Io("HSMS send cancelled".into())));
                    continue;
                }

                // NotifyTrySendHsmsMessagePassThrough (on send task, before wire).
                pt_s.put_try_send(&pack.msg);

                // Re-check cancel after try-send notify (timeout may race).
                if pack.cancelled.load(Ordering::SeqCst) {
                    let _ = pack.done.send(Err(Error::Io("HSMS send cancelled".into())));
                    continue;
                }

                match write_frame(&mut writer, &pack.msg) {
                    Ok(()) => {
                        act_s.touch();
                        pt_s.put_sended(&pack.msg);
                        let _ = pack.done.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = pack.done.send(Err(e));
                        // Fatal write: stop sending further (socket likely dead).
                        shut_s.store(true, Ordering::SeqCst);
                        tx_map_s.lock().expect("transaction map").clear();
                        while let Ok(p) = send_rx.try_recv() {
                            let _ = p.done.send(Err(Error::DetectTerminate));
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self {
            shutdown_stream: Mutex::new(shutdown_stream),
            send_tx: Mutex::new(Some(send_tx)),
            transactions,
            primary_rx: Mutex::new(primary_rx),
            shutdown,
            reader: Mutex::new(Some(reader)),
            sender: Mutex::new(Some(sender)),
            activity,
            pass_through,
        })
    }

    /// Enqueue one pack; returns write-completion oneshot + cancel flag.
    fn enqueue_send(
        &self,
        msg: HsmsMessage,
    ) -> Result<(Receiver<Result<()>>, Arc<AtomicBool>)> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(Error::ChannelShutdown);
        }
        let (done_tx, done_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let pack = SendPack {
            msg,
            done: done_tx,
            cancelled: Arc::clone(&cancelled),
        };
        let guard = self.send_tx.lock().expect("send_tx");
        let tx = guard.as_ref().ok_or(Error::ChannelShutdown)?;
        tx.send(pack).map_err(|_| Error::ChannelShutdown)?;
        Ok((done_rx, cancelled))
    }

    /// Block until the send task has written the pack (or fails / times out).
    ///
    /// Parity: `WaitUntilSended` under T3 (data) / T6 (control).
    /// `timeout == 0` means block until written (noreply callers often pass 0 for
    /// the reply wait slot only — the write itself must still complete).
    /// On timeout, `cancelled` is set so the sender drops the pack (no orphan write).
    fn wait_until_sended(
        done_rx: Receiver<Result<()>>,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        if timeout.is_zero() {
            return match done_rx.recv() {
                Ok(r) => r,
                Err(_) => Err(Error::DetectTerminate),
            };
        }
        match done_rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::SeqCst);
                Err(Error::Io("HSMS send timeout".into()))
            }
            Err(RecvTimeoutError::Disconnected) => Err(Error::DetectTerminate),
        }
    }

    /// Linktest idle-activity gate (shared with periodic linktest task).
    pub fn activity(&self) -> &Arc<LinktestActivity> {
        &self.activity
    }

    /// Pass-through facade for this session.
    pub fn pass_through(&self) -> &Arc<HsmsPassThrough> {
        &self.pass_through
    }

    /// Whether a reply is expected and which timeout class (T3 vs T6).
    ///
    /// Parity: DATA+W → T3; SELECT/DESELECT/LINKTEST req → T6; else no reply.
    pub fn reply_timeout_class(msg: &HsmsMessage) -> Option<ReplyTimeoutClass> {
        match msg.message_type() {
            HsmsMessageType::Data if msg.wbit() => Some(ReplyTimeoutClass::T3),
            HsmsMessageType::SelectReq
            | HsmsMessageType::DeselectReq
            | HsmsMessageType::LinktestReq => Some(ReplyTimeoutClass::T6),
            _ => None,
        }
    }

    /// Send message via the background send queue; if a reply is expected,
    /// block until reply or `timeout`.
    ///
    /// Flow (parity with Secs4Net `Send`):
    /// 1. register transaction (when reply expected)
    /// 2. `sendMsgQueue.Put` → wait until written (`WaitUntilSended`, same `timeout`)
    /// 3. wait reply (T3/T6) when applicable
    ///
    /// Returns `Ok(None)` when no reply is expected (e.g. DATA without W-bit).
    /// REJECT.req reply → `Error::Reject`.
    pub fn send(&self, msg: &HsmsMessage, timeout: Duration) -> Result<Option<HsmsMessage>> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(Error::ChannelShutdown);
        }

        let expect = Self::reply_timeout_class(msg);
        let key = msg.system_bytes_key();

        let reply_rx = if expect.is_some() {
            let (tx, rx) = mpsc::channel();
            self.transactions
                .lock()
                .expect("transaction map")
                .insert(key, tx);
            Some(rx)
        } else {
            None
        };

        // Enqueue owned clone; try-send/sended fire on the sender thread.
        let (done_rx, cancel) = match self.enqueue_send(msg.clone()) {
            Ok(x) => x,
            Err(e) => {
                if expect.is_some() {
                    self.transactions
                        .lock()
                        .expect("transaction map")
                        .remove(&key);
                }
                return Err(e);
            }
        };

        if let Err(e) = Self::wait_until_sended(done_rx, timeout, &cancel) {
            if expect.is_some() {
                self.transactions
                    .lock()
                    .expect("transaction map")
                    .remove(&key);
            }
            return Err(e);
        }

        let Some(rx) = reply_rx else {
            return Ok(None);
        };

        match rx.recv_timeout(timeout) {
            Ok(rsp) => {
                if rsp.message_type() == HsmsMessageType::RejectReq {
                    return Err(Error::Reject);
                }
                Ok(Some(rsp))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.transactions
                    .lock()
                    .expect("transaction map")
                    .remove(&key);
                match expect {
                    Some(ReplyTimeoutClass::T3) => Err(Error::TimeoutT3 {
                        primary: msg.clone(),
                    }),
                    Some(ReplyTimeoutClass::T6) | None => Err(Error::TimeoutT6),
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.transactions
                    .lock()
                    .expect("transaction map")
                    .remove(&key);
                Err(Error::DetectTerminate)
            }
        }
    }

    /// Send without registering a reply transaction (control auto-rsp, SEPARATE, …).
    ///
    /// Still goes through the send queue and blocks until written.
    pub fn send_noreply(&self, msg: &HsmsMessage) -> Result<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(Error::ChannelShutdown);
        }
        // Control / noreply path: generous wait (queue should drain promptly).
        let (done_rx, cancel) = self.enqueue_send(msg.clone())?;
        Self::wait_until_sended(done_rx, Duration::from_secs(30), &cancel)
    }

    /// Block until next primary (unsolicited) message.
    pub fn take_primary(&self) -> Result<HsmsMessage> {
        self.primary_rx
            .lock()
            .expect("primary_rx")
            .recv()
            .map_err(|_| Error::DetectTerminate)
    }

    /// Poll primary with timeout (`None` on timeout — T7-style poll).
    pub fn poll_primary(&self, timeout: Duration) -> Result<Option<HsmsMessage>> {
        match self
            .primary_rx
            .lock()
            .expect("primary_rx")
            .recv_timeout(timeout)
        {
            Ok(m) => Ok(Some(m)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(Error::DetectTerminate),
        }
    }

    /// Non-blocking try.
    pub fn try_primary(&self) -> Result<Option<HsmsMessage>> {
        match self.primary_rx.lock().expect("primary_rx").try_recv() {
            Ok(m) => Ok(Some(m)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Error::DetectTerminate),
        }
    }

    /// Active-side selected dispatch (SEMI-E37.1 SS Active MainTask).
    ///
    /// - DATA → `SelectedDispatch::Data`
    /// - LINKTEST_REQ → auto LINKTEST.rsp, continue
    /// - SELECT_REQ / DESELECT_REQ → REJECT NOT_SUPPORT_TYPE_S
    /// - SEPARATE_REQ → `SelectedDispatch::Separate`
    /// - response types → ignore / continue
    pub fn dispatch_active_primary(&self, primary: &HsmsMessage) -> Result<SelectedDispatch> {
        match primary.message_type() {
            HsmsMessageType::Data => Ok(SelectedDispatch::Data(primary.clone())),
            HsmsMessageType::SelectReq | HsmsMessageType::DeselectReq => {
                let r = build_reject_request(primary, RejectReason::NotSupportTypeS)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Continue)
            }
            HsmsMessageType::LinktestReq => {
                let r = build_linktest_response(primary)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Continue)
            }
            HsmsMessageType::SeparateReq => Ok(SelectedDispatch::Separate),
            _ => Ok(SelectedDispatch::Continue),
        }
    }

    /// Passive-side selected dispatch (SS Passive MainTask after SELECT success).
    ///
    /// - DATA → `SelectedDispatch::Data`
    /// - SELECT_REQ → SELECT.rsp ACTIVED
    /// - DESELECT_* → REJECT NOT_SUPPORT_TYPE_S
    /// - LINKTEST_REQ → LINKTEST.rsp
    /// - SEPARATE_REQ / REJECT_REQ → Separate (end session)
    /// - unexpected SELECT_RSP / LINKTEST_RSP → REJECT TRANSACTION_NOT_OPEN + Separate
    pub fn dispatch_passive_primary(&self, primary: &HsmsMessage) -> Result<SelectedDispatch> {
        use super::builder::build_select_response;
        use super::status::SelectStatus;

        match primary.message_type() {
            HsmsMessageType::Data => Ok(SelectedDispatch::Data(primary.clone())),
            HsmsMessageType::SelectReq => {
                let r = build_select_response(primary, SelectStatus::Actived)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Continue)
            }
            HsmsMessageType::DeselectReq | HsmsMessageType::DeselectRsp => {
                let r = build_reject_request(primary, RejectReason::NotSupportTypeS)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Continue)
            }
            HsmsMessageType::LinktestReq => {
                let r = build_linktest_response(primary)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Continue)
            }
            HsmsMessageType::SeparateReq | HsmsMessageType::RejectReq => {
                Ok(SelectedDispatch::Separate)
            }
            HsmsMessageType::SelectRsp | HsmsMessageType::LinktestRsp => {
                let r = build_reject_request(primary, RejectReason::TransactionNotOpen)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Separate)
            }
            _ => {
                // Unsupported type → reject and end (parity simplified).
                let reason = if HsmsMessageType::support_s_type(primary.s_type()) {
                    if HsmsMessageType::support_p_type(primary.p_type()) {
                        RejectReason::NotSupportTypeS
                    } else {
                        RejectReason::NotSupportTypeP
                    }
                } else {
                    RejectReason::NotSupportTypeS
                };
                let r = build_reject_request(primary, reason)?;
                self.send_noreply(&r)?;
                Ok(SelectedDispatch::Separate)
            }
        }
    }

    /// Shutdown channel (stops reader/sender after peer/local close).
    ///
    /// Clears the transaction map so pending reply waiters fail immediately
    /// (parity intent: link death must not hang T3/T6 callers until timeout).
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Drop reply oneshots first so T3/T6 waiters wake before socket I/O.
        self.transactions
            .lock()
            .expect("transaction map")
            .clear();
        // Close send queue so the sender loop can exit once drained/flagged.
        if let Ok(mut g) = self.send_tx.lock() {
            g.take();
        }
        if let Ok(s) = self.shutdown_stream.lock() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for HsmsSessionIo {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(h) = self.sender.get_mut().expect("sender").take() {
            let _ = h.join();
        }
        if let Some(h) = self.reader.get_mut().expect("reader").take() {
            let _ = h.join();
        }
    }
}

/// Reply timeout class (maps to T3 or T6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyTimeoutClass {
    T3,
    T6,
}

/// Message types that can complete a waiting send transaction.
///
/// DATA only completes when **not** W-bit (secondary / fire-and-forget reply shape).
/// A peer primary with W-bit must not steal a T3 waiter (system-bytes collision).
fn is_transaction_reply(msg: &HsmsMessage) -> bool {
    match msg.message_type() {
        HsmsMessageType::Data => !msg.wbit(),
        HsmsMessageType::SelectRsp
        | HsmsMessageType::DeselectRsp
        | HsmsMessageType::LinktestRsp
        | HsmsMessageType::RejectReq => true,
        _ => false,
    }
}

/// Result of selected-state primary dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectedDispatch {
    /// SECS data primary for the application.
    Data(HsmsMessage),
    /// Control handled (linktest/reject/etc.); keep looping.
    Continue,
    /// SEPARATE or fatal control; end selected session.
    Separate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::builder::{
        build_linktest_request, build_select_request, build_separate_request, SystemBytesCounter,
    };
    use crate::hsms::select::{active_select, passive_select};
    use crate::hsms::HsmsTcpChannel;
    use crate::secs2::Secs2;
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server, _) = listener.accept().unwrap();
        let client = client.join().unwrap();
        (server, client)
    }

    fn select_pair() -> (HsmsSessionIo, HsmsSessionIo) {
        let (server, client) = loopback_pair();
        // Select on raw channels first, then promote to SessionIo.
        // (Select must complete before reader thread steals frames.)
        let mut p_ch = HsmsTcpChannel::new(server.try_clone().unwrap());
        let mut a_ch = HsmsTcpChannel::new(client.try_clone().unwrap());
        let sys = SystemBytesCounter::new().next(false, 10);

        let p_sel = thread::spawn(move || {
            passive_select(&mut p_ch, Duration::from_secs(2)).unwrap();
            // drop channel clone after select
        });
        assert!(active_select(&mut a_ch, sys, Duration::from_secs(2)).unwrap());
        p_sel.join().unwrap();
        drop(a_ch);

        let passive = HsmsSessionIo::new(server).unwrap();
        let active = HsmsSessionIo::new(client).unwrap();
        (passive, active)
    }

    #[test]
    fn linktest_transaction_roundtrip() {
        let (passive, active) = select_pair();

        let p = thread::spawn(move || {
            // One primary: LINKTEST_REQ → auto rsp
            let msg = passive.take_primary().unwrap();
            let d = passive.dispatch_active_primary(&msg).unwrap();
            assert_eq!(d, SelectedDispatch::Continue);
            passive
        });

        let sys = [0, 0, 0, 9];
        let req = build_linktest_request(sys).unwrap();
        let rsp = active
            .send(&req, Duration::from_secs(2))
            .unwrap()
            .expect("linktest rsp");
        assert_eq!(rsp.message_type(), HsmsMessageType::LinktestRsp);
        assert_eq!(rsp.system_bytes_key(), req.system_bytes_key());

        p.join().unwrap();
    }

    #[test]
    fn data_primary_delivery() {
        let (passive, active) = select_pair();

        // DATA without W-bit → no reply wait; delivered as primary.
        let header_no_w = [
            0x00, 0x0A, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x42,
        ];
        let msg = HsmsMessage::of_with_body(&header_no_w, Secs2::ascii("HI").unwrap()).unwrap();
        assert!(HsmsSessionIo::reply_timeout_class(&msg).is_none());

        active.send(&msg, Duration::from_secs(1)).unwrap();

        let got = passive.take_primary().unwrap();
        assert!(got.is_data_message());
        assert_eq!(got.secs2().get_ascii().unwrap(), "HI");

        let d = passive.dispatch_passive_primary(&got).unwrap();
        match d {
            SelectedDispatch::Data(m) => {
                assert_eq!(m.get_stream(), 1);
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn separate_ends_selected() {
        let (passive, active) = select_pair();

        let p = thread::spawn(move || {
            let msg = passive.take_primary().unwrap();
            let d = passive.dispatch_passive_primary(&msg).unwrap();
            assert_eq!(d, SelectedDispatch::Separate);
        });

        let sep = build_separate_request([0, 0, 0, 3]).unwrap();
        active.send(&sep, Duration::from_secs(1)).unwrap();
        p.join().unwrap();
    }

    #[test]
    fn t6_timeout_on_linktest_no_reply() {
        let (_passive, active) = select_pair();
        // Passive does not dispatch → T6
        let req = build_linktest_request([0, 0, 0, 7]).unwrap();
        let err = active
            .send(&req, Duration::from_millis(80))
            .unwrap_err();
        assert_eq!(err, Error::TimeoutT6);
    }

    #[test]
    fn reply_timeout_class_parity() {
        let sel = build_select_request([0, 0, 0, 1]).unwrap();
        assert_eq!(
            HsmsSessionIo::reply_timeout_class(&sel),
            Some(ReplyTimeoutClass::T6)
        );
        let link = build_linktest_request([0, 0, 0, 2]).unwrap();
        assert_eq!(
            HsmsSessionIo::reply_timeout_class(&link),
            Some(ReplyTimeoutClass::T6)
        );
        let data_w = HsmsMessage::of(&[
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ])
        .unwrap();
        assert_eq!(
            HsmsSessionIo::reply_timeout_class(&data_w),
            Some(ReplyTimeoutClass::T3)
        );
        let data = HsmsMessage::of(&[
            0x00, 0x0A, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ])
        .unwrap();
        assert!(HsmsSessionIo::reply_timeout_class(&data).is_none());
    }

    #[test]
    fn linktest_activity_wait_idle_and_touch() {
        let act = LinktestActivity::new();
        let stop = AtomicBool::new(false);
        // Short idle completes
        assert!(act.wait_idle(Duration::from_millis(40), &stop));
        // Touch during wait → false
        let act2 = Arc::new(LinktestActivity::new());
        let a = Arc::clone(&act2);
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(15));
            a.touch();
        });
        assert!(!act2.wait_idle(Duration::from_millis(200), &stop));
        th.join().unwrap();
    }

    #[test]
    fn send_touches_linktest_activity() {
        let (passive, active) = select_pair();
        let stop = AtomicBool::new(false);
        // Start idle wait on active activity; peer DATA receive + our send should touch.
        let act = Arc::clone(active.activity());
        let waiter = thread::spawn(move || {
            // Would complete only if no I/O for 300ms
            act.wait_idle(Duration::from_millis(300), &stop)
        });
        thread::sleep(Duration::from_millis(20));
        // Active sends DATA → touch
        active
            .send(
                &crate::hsms::build_data_message(
                    10,
                    1,
                    1,
                    false,
                    Secs2::ascii("X").unwrap(),
                    [0, 0, 0, 9],
                )
                .unwrap(),
                Duration::from_secs(1),
            )
            .unwrap();
        // Drain passive primary so reader path also touches
        let _ = passive.poll_primary(Duration::from_millis(200));
        assert!(
            !waiter.join().unwrap(),
            "send must reset linktest idle timer"
        );
    }

    /// Concurrent `send` all complete via the queue; peer gets every message.
    #[test]
    fn concurrent_send_queue_all_delivered() {
        use std::collections::HashSet;

        let (passive, active) = select_pair();
        let active = Arc::new(active);

        let n = 8u8;
        let mut handles = Vec::new();
        for i in 0..n {
            let a = Arc::clone(&active);
            handles.push(thread::spawn(move || {
                let msg = crate::hsms::build_data_message(
                    10,
                    1,
                    1,
                    false,
                    Secs2::ascii(&format!("M{i}")).unwrap(),
                    [0, 0, 0, i],
                )
                .unwrap();
                a.send(&msg, Duration::from_secs(2)).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let mut keys = HashSet::new();
        for _ in 0..n {
            let m = passive
                .poll_primary(Duration::from_secs(2))
                .unwrap()
                .expect("primary DATA");
            keys.insert(m.system_bytes_key());
        }
        let expected: HashSet<i64> = (0..n).map(i64::from).collect();
        assert_eq!(keys, expected, "all concurrent sends must be delivered");
    }

    /// Sequential enqueue order is preserved on the wire (FIFO send queue).
    #[test]
    fn send_queue_fifo_order() {
        let (passive, active) = select_pair();
        let n = 5u8;
        for i in 0..n {
            let msg = crate::hsms::build_data_message(
                10,
                1,
                1,
                false,
                Secs2::ascii(&format!("S{i}")).unwrap(),
                [0, 0, 0, i],
            )
            .unwrap();
            active.send(&msg, Duration::from_secs(2)).unwrap();
        }
        let mut keys = Vec::new();
        for _ in 0..n {
            let m = passive
                .poll_primary(Duration::from_secs(2))
                .unwrap()
                .expect("primary");
            keys.push(m.system_bytes_key());
        }
        assert_eq!(keys, (0..n).map(i64::from).collect::<Vec<_>>());
    }

    #[test]
    fn send_after_shutdown_is_channel_shutdown() {
        let (passive, active) = select_pair();
        active.shutdown();
        // Give sender/reader a moment to observe the flag.
        thread::sleep(Duration::from_millis(20));
        let msg = crate::hsms::build_data_message(
            10,
            1,
            1,
            false,
            Secs2::ascii("Z").unwrap(),
            [0, 0, 0, 1],
        )
        .unwrap();
        let err = active.send(&msg, Duration::from_millis(200)).unwrap_err();
        assert_eq!(err, Error::ChannelShutdown);
        drop(passive);
    }

    /// Idle Selected link must survive longer than T8 without traffic.
    ///
    /// Regression: permanent socket read_timeout=T8 tore down quiet sessions ~T8,
    /// then Active T5-retry / Passive rebind looked like flapping reconnects.
    #[test]
    fn idle_selected_survives_beyond_t8() {
        let (passive, active) = select_pair();
        // T8 default in with_pass_through is 6s; wait past that with no I/O.
        thread::sleep(Duration::from_millis(6500));
        let msg = crate::hsms::build_data_message(
            10,
            1,
            1,
            false,
            Secs2::ascii("AFTER-IDLE").unwrap(),
            [0, 0, 0, 42],
        )
        .unwrap();
        active
            .send(&msg, Duration::from_secs(2))
            .expect("send after idle must still work");
        let got = passive
            .poll_primary(Duration::from_secs(2))
            .unwrap()
            .expect("primary after idle");
        assert_eq!(got.secs2().get_ascii().unwrap(), "AFTER-IDLE");
    }

    #[test]
    fn shutdown_unblocks_pending_reply_wait() {
        // Pending DATA+W must fail promptly when channel shuts down (no T3 hang).
        let (passive, active) = select_pair();
        let a = Arc::new(active);
        let a2 = Arc::clone(&a);
        let send = thread::spawn(move || {
            let msg = crate::hsms::build_data_message(
                10,
                1,
                1,
                true,
                Secs2::ascii("WAIT").unwrap(),
                [0, 0, 0, 9],
            )
            .unwrap();
            a2.send(&msg, Duration::from_secs(30))
        });

        // Ensure primary is delivered; peer never replies.
        let _ = passive.poll_primary(Duration::from_millis(500));
        thread::sleep(Duration::from_millis(50));
        a.shutdown();

        let err = send
            .join()
            .expect("join")
            .expect_err("shutdown must fail waiter");
        assert!(
            matches!(
                err,
                Error::DetectTerminate | Error::ChannelShutdown | Error::TimeoutT3 { .. }
            ),
            "unexpected {err:?}"
        );
        drop(passive);
    }

    #[test]
    fn peer_drop_unblocks_pending_reply_wait() {
        let (passive, active) = select_pair();
        let a = Arc::new(active);
        let a2 = Arc::clone(&a);
        let send = thread::spawn(move || {
            let msg = crate::hsms::build_data_message(
                10,
                1,
                1,
                true,
                Secs2::ascii("PEER").unwrap(),
                [0, 0, 0, 8],
            )
            .unwrap();
            a2.send(&msg, Duration::from_secs(30))
        });

        let _ = passive.poll_primary(Duration::from_millis(500));
        thread::sleep(Duration::from_millis(50));
        // Drop peer → TCP close → reader exit → transaction map clear.
        drop(passive);

        let err = send
            .join()
            .expect("join")
            .expect_err("peer drop must fail waiter");
        assert!(
            matches!(
                err,
                Error::DetectTerminate | Error::ChannelShutdown | Error::TimeoutT3 { .. }
            ),
            "unexpected {err:?}"
        );
        drop(a);
    }
}
