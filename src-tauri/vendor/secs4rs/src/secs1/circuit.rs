//! SECS-I circuit: byte queue + send completion + T3 transaction map.
//!
//! Source: `AbstractSecs1CircuitFacade` (idiomatic queue/thread shape).
//! Full log observers / device-id filter deferred.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::block::Secs1MessageBlock;
use super::error::{Error, Result};
use super::message::Secs1Message;
use super::wire_io::{ACK, ENQ, EOT, NAK};

/// Runtime timeouts / role for one circuit session.
#[derive(Debug, Clone)]
pub struct Secs1CircuitConfig {
    /// Inter-character timeout within a block body (SEMI T1, default 1s).
    pub t1: Duration,
    /// Protocol handshake / first length byte after EOT (SEMI T2, default 15s).
    pub t2: Duration,
    pub t3: Duration,
    pub t4: Duration,
    pub is_master: bool,
    pub retry: u32,
    /// Local Device-ID (for block filter).
    pub device_id: i32,
    /// When true, drop blocks whose Device-ID ≠ `device_id` after ACK
    /// (`IsCheckMessageBlockDeviceId`, default true).
    pub check_block_device_id: bool,
}

impl Default for Secs1CircuitConfig {
    fn default() -> Self {
        Self {
            t1: Duration::from_secs(1),
            t2: Duration::from_secs(15),
            t3: Duration::from_secs(45),
            t4: Duration::from_secs(45),
            is_master: true,
            retry: 3,
            device_id: 10,
            check_block_device_id: true,
        }
    }
}

fn system_bytes_key_msg(msg: &Secs1Message) -> i32 {
    let bs = msg.header10_bytes();
    (i32::from(bs[6]) << 24)
        | (i32::from(bs[7]) << 16)
        | (i32::from(bs[8]) << 8)
        | i32::from(bs[9])
}

fn system_bytes_key_block(block: &Secs1MessageBlock) -> i32 {
    let bs = block.get_bytes();
    if bs.len() < 11 {
        return 0;
    }
    (i32::from(bs[7]) << 24)
        | (i32::from(bs[8]) << 16)
        | (i32::from(bs[9]) << 8)
        | i32::from(bs[10])
}

struct BlockPack {
    message: Secs1Message,
    blocks: Vec<Secs1MessageBlock>,
    present: usize,
}

impl BlockPack {
    fn new(message: Secs1Message) -> Self {
        let blocks = message.to_blocks().to_vec();
        Self {
            message,
            blocks,
            present: 0,
        }
    }

    fn present(&self) -> &Secs1MessageBlock {
        &self.blocks[self.present]
    }

    fn reset(&mut self) {
        self.present = 0;
    }

    fn next(&mut self) {
        self.present += 1;
    }

    fn ebit(&self) -> bool {
        self.blocks[self.present].ebit()
    }
}

enum ByteOrMsg {
    Byte(u8),
    Msg(BlockPack),
}

struct QueueInner {
    bytes: VecDeque<u8>,
    msgs: VecDeque<BlockPack>,
    closed: bool,
}

struct ByteMsgQueue {
    inner: Mutex<QueueInner>,
    cv: Condvar,
}

impl ByteMsgQueue {
    fn new() -> Self {
        Self {
            inner: Mutex::new(QueueInner {
                bytes: VecDeque::new(),
                msgs: VecDeque::new(),
                closed: false,
            }),
            cv: Condvar::new(),
        }
    }

    fn put_bytes(&self, bs: &[u8]) {
        let mut g = self.inner.lock().expect("queue");
        if g.closed {
            return;
        }
        g.bytes.extend(bs.iter().copied());
        self.cv.notify_all();
    }

    /// Enqueue outbound pack. Returns `false` if the queue is already closed
    /// (caller must fail the send slot — never drop a registered send silently).
    fn put_message(&self, pack: BlockPack) -> bool {
        let mut g = self.inner.lock().expect("queue");
        if g.closed {
            return false;
        }
        g.msgs.push_back(pack);
        self.cv.notify_all();
        true
    }

    fn close(&self) {
        let mut g = self.inner.lock().expect("queue");
        g.closed = true;
        // Drop unsent packs — send managers are failed separately in `Secs1Circuit::close`.
        g.msgs.clear();
        self.cv.notify_all();
    }

    /// Outgoing messages take priority over received bytes.
    fn take_byte_or_msg(&self) -> Result<ByteOrMsg> {
        let mut g = self.inner.lock().expect("queue");
        loop {
            if !g.msgs.is_empty() {
                return Ok(ByteOrMsg::Msg(g.msgs.pop_front().unwrap()));
            }
            if let Some(b) = g.bytes.pop_front() {
                return Ok(ByteOrMsg::Byte(b));
            }
            if g.closed {
                return Err(Error::ChannelShutdown);
            }
            g = self.cv.wait(g).expect("queue wait");
        }
    }

    fn poll_byte(&self, timeout: Duration) -> Option<u8> {
        let mut g = self.inner.lock().expect("queue");
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(b) = g.bytes.pop_front() {
                return Some(b);
            }
            if g.closed {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (ng, _) = self
                .cv
                .wait_timeout(g, deadline - now)
                .expect("queue wait_timeout");
            g = ng;
        }
    }

    fn poll_bytes(&self, buf: &mut [u8], timeout: Duration) -> usize {
        let mut n = 0;
        while n < buf.len() {
            match self.poll_byte(timeout) {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }

    fn garbage_bytes(&self, timeout: Duration) {
        {
            let mut g = self.inner.lock().expect("queue");
            g.bytes.clear();
        }
        while self.poll_byte(timeout).is_some() {}
    }
}

#[derive(Default)]
struct SendSlot {
    done: bool,
    err: Option<Error>,
}

struct SendManager {
    map: Mutex<HashMap<i32, Arc<(Mutex<SendSlot>, Condvar)>>>,
}

impl SendManager {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    fn enter(&self, key: i32) -> Arc<(Mutex<SendSlot>, Condvar)> {
        let slot = Arc::new((Mutex::new(SendSlot::default()), Condvar::new()));
        self.map.lock().expect("send map").insert(key, Arc::clone(&slot));
        slot
    }

    fn exit(&self, key: i32) {
        self.map.lock().expect("send map").remove(&key);
    }

    fn put_sended(&self, key: i32) {
        if let Some(slot) = self.map.lock().expect("send map").get(&key) {
            let mut g = slot.0.lock().expect("send slot");
            g.done = true;
            slot.1.notify_all();
        }
    }

    fn put_exception(&self, key: i32, e: Error) {
        if let Some(slot) = self.map.lock().expect("send map").get(&key) {
            let mut g = slot.0.lock().expect("send slot");
            g.err = Some(e);
            slot.1.notify_all();
        }
    }

    /// Fail every outstanding send waiter (close / channel death).
    fn fail_all(&self, e: Error) {
        let map = self.map.lock().expect("send map");
        for slot in map.values() {
            let mut g = slot.0.lock().expect("send slot");
            if !g.done && g.err.is_none() {
                g.err = Some(e.clone());
                slot.1.notify_all();
            }
        }
    }

    fn wait_until_sended(slot: &Arc<(Mutex<SendSlot>, Condvar)>) -> Result<()> {
        let mut g = slot.0.lock().expect("send slot");
        loop {
            if g.done {
                return Ok(());
            }
            if let Some(ref e) = g.err {
                return Err(e.clone());
            }
            g = slot.1.wait(g).expect("send wait");
        }
    }
}

struct ReplySlot {
    reply: Option<Secs1Message>,
    timer_reset: bool,
    /// Circuit closed while waiting for reply.
    closed: bool,
}

struct TransManager {
    map: Mutex<HashMap<i32, Arc<(Mutex<ReplySlot>, Condvar)>>>,
}

impl TransManager {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    fn enter(&self, key: i32) -> Arc<(Mutex<ReplySlot>, Condvar)> {
        let slot = Arc::new((
            Mutex::new(ReplySlot {
                reply: None,
                timer_reset: false,
                closed: false,
            }),
            Condvar::new(),
        ));
        self.map.lock().expect("trans map").insert(key, Arc::clone(&slot));
        slot
    }

    fn exit(&self, key: i32) {
        self.map.lock().expect("trans map").remove(&key);
    }

    /// Deliver reply if key matches a pending transaction; else return as primary.
    fn put(&self, msg: Secs1Message) -> Option<Secs1Message> {
        let key = system_bytes_key_msg(&msg);
        let slot = {
            let g = self.map.lock().expect("trans map");
            g.get(&key).cloned()
        };
        if let Some(slot) = slot {
            let mut g = slot.0.lock().expect("reply slot");
            g.reply = Some(msg);
            slot.1.notify_all();
            None
        } else {
            Some(msg)
        }
    }

    fn reset_timer(&self, block: &Secs1MessageBlock) {
        let key = system_bytes_key_block(block);
        if let Some(slot) = self.map.lock().expect("trans map").get(&key) {
            let mut g = slot.0.lock().expect("reply slot");
            g.timer_reset = true;
            slot.1.notify_all();
        }
    }

    /// Fail every outstanding T3 waiter (close / channel death).
    fn fail_all(&self) {
        let map = self.map.lock().expect("trans map");
        for slot in map.values() {
            let mut g = slot.0.lock().expect("reply slot");
            g.closed = true;
            slot.1.notify_all();
        }
    }

    fn wait_reply(
        slot: &Arc<(Mutex<ReplySlot>, Condvar)>,
        t3: Duration,
    ) -> Result<Secs1Message> {
        let mut g = slot.0.lock().expect("reply slot");
        if let Some(r) = g.reply.take() {
            return Ok(r);
        }
        if g.closed {
            return Err(Error::ChannelShutdown);
        }
        // Absolute deadline: multi-block `timer_reset` restarts T3; spurious wakes do not.
        let mut deadline = Instant::now() + t3;
        loop {
            if g.closed {
                return Err(Error::ChannelShutdown);
            }
            let now = Instant::now();
            if now >= deadline {
                if let Some(r) = g.reply.take() {
                    return Ok(r);
                }
                return Err(Error::TimeoutT3);
            }
            let rem = deadline.saturating_duration_since(now);
            let (ng, _to) = slot.1.wait_timeout(g, rem).expect("reply wait");
            g = ng;
            if let Some(r) = g.reply.take() {
                return Ok(r);
            }
            if g.closed {
                return Err(Error::ChannelShutdown);
            }
            if g.timer_reset {
                g.timer_reset = false;
                deadline = Instant::now() + t3;
            }
            // Spurious wake or partial timeout: keep waiting until absolute deadline.
        }
    }
}

/// One attached TCP channel (multi-accept path): write half + id for removal.
struct ChannelWriter {
    id: u64,
    stream: TcpStream,
}

/// Connected SECS-I circuit session (reader + protocol loop + T3 map).
///
/// Single-stream: [`Self::start`]. Multi-channel receiver: [`Self::start_multi`]
/// + [`Self::attach_channel`] (parity: C# `AddChannel` / concurrent `HandleAccepted`).
pub struct Secs1Circuit {
    queue: Arc<ByteMsgQueue>,
    send_mgr: Arc<SendManager>,
    trans_mgr: Arc<TransManager>,
    /// Outbound channels; send uses index 0 (`GetChannel` → first).
    writers: Mutex<Vec<ChannelWriter>>,
    config: Secs1CircuitConfig,
    primary_tx: Mutex<Option<Sender<Secs1Message>>>,
    primary_rx: Mutex<Receiver<Secs1Message>>,
    shutdown: Arc<AtomicBool>,
    /// True while circuit loop is running (false after fatal queue close or close()).
    alive: Arc<AtomicBool>,
    /// Multi-accept: do not close the byte queue when a single reader exits.
    multi: bool,
    /// Attached channel count (multi path / single after start).
    channel_count: AtomicUsize,
    next_channel_id: AtomicU64,
    readers: Mutex<Vec<JoinHandle<()>>>,
    circuit: Mutex<Option<JoinHandle<()>>>,
}

impl Secs1Circuit {
    fn spawn_circuit_loop(this: &Arc<Self>) {
        let circuit_self = Arc::clone(this);
        let circuit = thread::spawn(move || {
            while !circuit_self.shutdown.load(Ordering::SeqCst) {
                if circuit_self.enter_once().is_err() {
                    break;
                }
            }
            // Queue closed / fatal protocol exit: same as peer drop for waiters.
            circuit_self.fail_pending_waiters();
        });
        *this.circuit.lock().expect("circuit handle") = Some(circuit);
    }

    fn new_shell(config: Secs1CircuitConfig, multi: bool) -> Arc<Self> {
        let queue = Arc::new(ByteMsgQueue::new());
        let send_mgr = Arc::new(SendManager::new());
        let trans_mgr = Arc::new(TransManager::new());
        let (primary_tx, primary_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));

        Arc::new(Self {
            queue,
            send_mgr,
            trans_mgr,
            writers: Mutex::new(Vec::new()),
            config,
            primary_tx: Mutex::new(Some(primary_tx)),
            primary_rx: Mutex::new(primary_rx),
            shutdown,
            alive,
            multi,
            channel_count: AtomicUsize::new(0),
            next_channel_id: AtomicU64::new(1),
            readers: Mutex::new(Vec::new()),
            circuit: Mutex::new(None),
        })
    }

    /// Take ownership of a connected stream; spawn reader + circuit threads.
    pub fn start(stream: TcpStream, config: Secs1CircuitConfig) -> Result<Arc<Self>> {
        let writer = stream.try_clone().map_err(Error::from)?;
        let mut reader_stream = stream;

        let this = Self::new_shell(config, false);
        {
            let id = this.next_channel_id.fetch_add(1, Ordering::SeqCst);
            this.writers.lock().expect("writers").push(ChannelWriter {
                id,
                stream: writer,
            });
            this.channel_count.store(1, Ordering::SeqCst);
        }

        let this_r = Arc::clone(&this);
        let reader = thread::spawn(move || {
            let _ = reader_stream.set_read_timeout(Some(Duration::from_millis(200)));
            let mut buf = [0u8; 512];
            while !this_r.shutdown.load(Ordering::SeqCst) {
                match reader_stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => this_r.queue.put_bytes(&buf[..n]),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(_) => break,
                }
            }
            // Single-stream: peer drop ends the circuit — fail waiters (no hang).
            this_r.channel_count.store(0, Ordering::SeqCst);
            this_r.fail_pending_waiters();
            this_r.queue.close();
        });
        this.readers.lock().expect("readers").push(reader);
        Self::spawn_circuit_loop(&this);
        Ok(this)
    }

    /// Multi-accept shell: circuit loop only; attach sockets via [`Self::attach_channel`].
    ///
    /// Readers never close the shared queue (parity: many TCP channels → one PutBytes).
    pub fn start_multi(config: Secs1CircuitConfig) -> Arc<Self> {
        let this = Self::new_shell(config, true);
        Self::spawn_circuit_loop(&this);
        this
    }

    /// Register an accepted connection: reader → shared queue; write half at list end.
    ///
    /// Send uses the first channel (`GetChannel` → `_channels[0]`).
    pub fn attach_channel(self: &Arc<Self>, stream: TcpStream) -> Result<u64> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(Error::ChannelShutdown);
        }
        let writer = stream.try_clone().map_err(Error::from)?;
        let mut reader_stream = stream;
        let id = self.next_channel_id.fetch_add(1, Ordering::SeqCst);

        self.writers
            .lock()
            .expect("writers")
            .push(ChannelWriter { id, stream: writer });
        self.channel_count.fetch_add(1, Ordering::SeqCst);

        let this = Arc::clone(self);
        let reader = thread::spawn(move || {
            let _ = reader_stream.set_read_timeout(Some(Duration::from_millis(200)));
            let mut buf = [0u8; 512];
            while !this.shutdown.load(Ordering::SeqCst) {
                match reader_stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => this.queue.put_bytes(&buf[..n]),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(_) => break,
                }
            }
            // RemoveChannel: drop write half; do not close shared queue.
            this.detach_channel(id);
        });
        self.readers.lock().expect("readers").push(reader);
        Ok(id)
    }

    fn detach_channel(&self, id: u64) {
        let mut w = self.writers.lock().expect("writers");
        if let Some(pos) = w.iter().position(|c| c.id == id) {
            w.remove(pos);
            self.channel_count.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Whether the circuit loop is still running (false after fatal close).
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && !self.shutdown.load(Ordering::SeqCst)
    }

    /// Number of attached TCP channels (`_channels.Count`).
    pub fn channel_count(&self) -> usize {
        self.channel_count.load(Ordering::SeqCst)
    }

    /// At least one TCP channel is attached.
    pub fn has_channel(&self) -> bool {
        self.channel_count() > 0
    }

    fn send_bytes(&self, bs: &[u8]) -> Result<()> {
        let mut writers = self.writers.lock().expect("writers");
        let w = writers
            .first_mut()
            .ok_or(Error::ChannelShutdown)?;
        w.stream.write_all(bs).map_err(Error::from)?;
        w.stream.flush().map_err(Error::from)?;
        Ok(())
    }

    fn send_byte(&self, b: u8) -> Result<()> {
        self.send_bytes(&[b])
    }

    /// Send a SECS-I message; if W-bit set, wait for reply (T3).
    pub fn send(&self, msg: Secs1Message) -> Result<Option<Secs1Message>> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(Error::ChannelShutdown);
        }
        let key = system_bytes_key_msg(&msg);
        let wait_reply = msg.wbit() && msg.is_valid_blocks();
        let send_slot = self.send_mgr.enter(key);
        let reply_slot = if wait_reply {
            Some(self.trans_mgr.enter(key))
        } else {
            None
        };
        let result = (|| {
            if !self.queue.put_message(BlockPack::new(msg)) {
                return Err(Error::ChannelShutdown);
            }
            SendManager::wait_until_sended(&send_slot)?;
            if let Some(ref rs) = reply_slot {
                let reply = TransManager::wait_reply(rs, self.config.t3)?;
                Ok(Some(reply))
            } else {
                Ok(None)
            }
        })();
        // Always leave maps (success, RetryOver, T3, shutdown, …).
        self.send_mgr.exit(key);
        if reply_slot.is_some() {
            self.trans_mgr.exit(key);
        }
        result
    }

    /// Poll a received primary message (not a matched reply).
    pub fn poll_primary(&self, timeout: Duration) -> Result<Option<Secs1Message>> {
        let rx = self.primary_rx.lock().expect("primary rx");
        match rx.recv_timeout(timeout) {
            Ok(m) => Ok(Some(m)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::ChannelShutdown),
        }
    }

    /// Block until a primary message arrives.
    pub fn recv_primary(&self) -> Result<Secs1Message> {
        let rx = self.primary_rx.lock().expect("primary rx");
        rx.recv().map_err(|_| Error::ChannelShutdown)
    }

    /// Fail outstanding send / T3 / primary waiters (peer drop or local close).
    ///
    /// Safe to call more than once. Does not set `shutdown` (caller may).
    fn fail_pending_waiters(&self) {
        self.alive.store(false, Ordering::SeqCst);
        self.send_mgr.fail_all(Error::ChannelShutdown);
        self.trans_mgr.fail_all();
        {
            let mut tx = self.primary_tx.lock().expect("primary tx");
            *tx = None;
        }
    }

    pub fn close(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Wake pending send / T3 waiters before closing the queue (avoid hang).
        self.fail_pending_waiters();
        self.queue.close();
        if let Ok(mut writers) = self.writers.lock() {
            for w in writers.drain(..) {
                let _ = w.stream.shutdown(std::net::Shutdown::Both);
            }
        }
        self.channel_count.store(0, Ordering::SeqCst);
        // Drain reader handles (may still be exiting).
        let handles: Vec<_> = self.readers.lock().expect("readers").drain(..).collect();
        for h in handles {
            let _ = h.join();
        }
        if let Some(h) = self.circuit.lock().expect("circuit").take() {
            let _ = h.join();
        }
    }

    fn deliver_primary(&self, msg: Secs1Message) {
        if let Some(tx) = self.primary_tx.lock().expect("primary tx").as_ref() {
            let _ = tx.send(msg);
        }
    }

    fn enter_once(&self) -> Result<()> {
        match self.queue.take_byte_or_msg()? {
            ByteOrMsg::Byte(b) => {
                if b == ENQ {
                    if let Err(e) = self.receiving() {
                        // Log-equivalent: swallow protocol errors so the loop continues.
                        let _ = e;
                    }
                }
                Ok(())
            }
            ByteOrMsg::Msg(mut pack) => {
                let key = system_bytes_key_msg(&pack.message);
                match self.send_pack(&mut pack) {
                    Ok(()) => {
                        self.send_mgr.put_sended(key);
                        Ok(())
                    }
                    Err(e) => {
                        self.send_mgr.put_exception(key, e.clone());
                        // Keep circuit alive after send failure.
                        let _ = e;
                        Ok(())
                    }
                }
            }
        }
    }

    fn send_pack(&self, pack: &mut BlockPack) -> Result<()> {
        let mut attempts = 0u32;
        while attempts <= self.config.retry {
            self.send_byte(ENQ)?;
            loop {
                match self.queue.poll_byte(self.config.t2) {
                    None => {
                        attempts += 1;
                        break;
                    }
                    Some(b) if b == ENQ && !self.config.is_master => {
                        // Slave yield: receive peer message, reset send cursor.
                        let _ = self.receiving();
                        pack.reset();
                        attempts = 0;
                        break;
                    }
                    Some(b) if b == ENQ && self.config.is_master => {
                        // Master keeps waiting for EOT within remaining budget via next poll.
                        continue;
                    }
                    Some(b) if b == EOT => {
                        if self.sending_block(pack.present())? {
                            if pack.ebit() {
                                return Ok(());
                            }
                            pack.next();
                            attempts = 0;
                            break;
                        } else {
                            attempts += 1;
                            break;
                        }
                    }
                    Some(_) => {
                        // Unexpected control: count as retry path.
                        attempts += 1;
                        break;
                    }
                }
            }
        }
        Err(Error::RetryOver)
    }

    fn sending_block(&self, block: &Secs1MessageBlock) -> Result<bool> {
        self.send_bytes(block.get_bytes())?;
        match self.queue.poll_byte(self.config.t2) {
            Some(b) if b == ACK => Ok(true),
            Some(_) => Ok(false),
            None => Ok(false),
        }
    }

    fn receiving(&self) -> Result<()> {
        self.send_byte(EOT)?;

        let mut bs = [0u8; 257];
        let r = self.queue.poll_bytes(&mut bs[..1], self.config.t2);
        if r <= 0 {
            let _ = self.send_byte(NAK);
            return Err(Error::TimeoutT2);
        }

        let len = usize::from(bs[0]);
        if !(10..=254).contains(&len) {
            self.queue.garbage_bytes(self.config.t2);
            let _ = self.send_byte(NAK);
            return Err(Error::IllegalLengthByte(len as i32));
        }

        let total = len + 3;
        let mut pos = 1usize;
        // Remaining block payload: inter-character T1 (not T2).
        while pos < total {
            let n = self.queue.poll_bytes(&mut bs[pos..total], self.config.t1);
            if n == 0 {
                let _ = self.send_byte(NAK);
                return Err(Error::TimeoutT1);
            }
            pos += n;
        }

        let block = Secs1MessageBlock::of(bs[..total].to_vec());
        if block.check_sum() {
            self.send_byte(ACK)?;
        } else {
            self.queue.garbage_bytes(self.config.t2);
            let _ = self.send_byte(NAK);
            return Err(Error::ChecksumMismatch);
        }

        // Device-ID filter after ACK (parity: drop silently, do not cache).
        if self.config.check_block_device_id && block.device_id() != self.config.device_id {
            return Ok(());
        }

        self.cache_and_finish(block)
    }

    fn cache_and_finish(&self, block: Secs1MessageBlock) -> Result<()> {
        // Single-shot cache on stack via recursive multi-block path.
        self.recv_accumulate(vec![block])
    }

    fn accept_block_device_id(&self, block: &Secs1MessageBlock) -> bool {
        !self.config.check_block_device_id || block.device_id() == self.config.device_id
    }

    fn recv_accumulate(&self, mut cache: Vec<Secs1MessageBlock>) -> Result<()> {
        let last = cache.last().expect("cache non-empty").clone();
        if last.ebit() {
            match Secs1Message::build_from_blocks(&cache) {
                Ok(s1msg) => {
                    if let Some(primary) = self.trans_mgr.put(s1msg) {
                        self.deliver_primary(primary);
                    }
                }
                Err(e) => return Err(e),
            }
            return Ok(());
        }

        // Intermediate block: reset T3 on matching transaction; wait next ENQ within T4.
        self.trans_mgr.reset_timer(&last);
        match self.queue.poll_byte(self.config.t4) {
            None => Err(Error::TimeoutT4),
            Some(b) if b == ENQ => {
                self.send_byte(EOT)?;
                let mut bs = [0u8; 257];
                let r = self.queue.poll_bytes(&mut bs[..1], self.config.t2);
                if r <= 0 {
                    let _ = self.send_byte(NAK);
                    return Err(Error::TimeoutT2);
                }
                let len = usize::from(bs[0]);
                if !(10..=254).contains(&len) {
                    self.queue.garbage_bytes(self.config.t2);
                    let _ = self.send_byte(NAK);
                    return Err(Error::IllegalLengthByte(len as i32));
                }
                let total = len + 3;
                let mut pos = 1usize;
                while pos < total {
                    let n = self.queue.poll_bytes(&mut bs[pos..total], self.config.t1);
                    if n == 0 {
                        let _ = self.send_byte(NAK);
                        return Err(Error::TimeoutT1);
                    }
                    pos += n;
                }
                let block = Secs1MessageBlock::of(bs[..total].to_vec());
                if block.check_sum() {
                    self.send_byte(ACK)?;
                } else {
                    self.queue.garbage_bytes(self.config.t2);
                    let _ = self.send_byte(NAK);
                    return Err(Error::ChecksumMismatch);
                }

                if !self.accept_block_device_id(&block) {
                    // Drop mismatched Device-ID block; keep waiting for next ENQ of current msg.
                    return self.recv_accumulate(cache);
                }

                // Append if same system-bytes and next block number; else restart cache.
                if let Some(prev) = cache.last() {
                    if prev.equals_system_bytes(&block) && prev.is_next_block(&block) {
                        cache.push(block);
                    } else if !prev.equals_system_bytes(&block) {
                        cache.clear();
                        cache.push(block);
                    }
                    // same system-bytes but wrong number: ignore (C# only appends on is_next)
                } else {
                    cache.push(block);
                }
                self.recv_accumulate(cache)
            }
            Some(b) => Err(Error::NotReceiveNextBlockEnq(b)),
        }
    }
}

impl Drop for Secs1Circuit {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secs2::Secs2;
    use std::net::TcpListener;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server, _) = listener.accept().unwrap();
        let client = client.join().unwrap();
        (server, client)
    }

    fn cfg_fast() -> Secs1CircuitConfig {
        Secs1CircuitConfig {
            t1: Duration::from_secs(1),
            t2: Duration::from_secs(2),
            t3: Duration::from_millis(500),
            t4: Duration::from_secs(2),
            is_master: true,
            retry: 3,
            device_id: 10,
            check_block_device_id: true,
        }
    }

    fn msg(sys: u8, wbit: bool, body: &str) -> Secs1Message {
        let s2 = if wbit { 0x81 } else { 0x01 };
        let header = [0x00, 0x0A, s2, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, sys];
        Secs1Message::build_data_message(&header, Secs2::ascii(body).unwrap()).unwrap()
    }

    fn msg_device(device: i32, sys: u8, body: &str) -> Secs1Message {
        let header = [
            ((device >> 8) & 0x7F) as u8,
            (device & 0xFF) as u8,
            0x01,
            0x01,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            sys,
        ];
        Secs1Message::build_data_message(&header, Secs2::ascii(body).unwrap()).unwrap()
    }

    fn reply_to(primary: &Secs1Message, body: &str) -> Secs1Message {
        let mut h = primary.header10_bytes();
        h[2] = 0x01; // stream 1, no W
        h[3] = 0x02; // function 2
        Secs1Message::build_data_message(&h, Secs2::ascii(body).unwrap()).unwrap()
    }

    #[test]
    fn circuit_send_recv_primary() {
        let (a, b) = loopback_pair();
        let ca = Secs1Circuit::start(a, cfg_fast()).unwrap();
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let m = msg(0x31, false, "HI");
        let send = thread::spawn({
            let ca = Arc::clone(&ca);
            let m = m.clone();
            move || ca.send(m).unwrap()
        });

        let got = cb.recv_primary().unwrap();
        assert_eq!(got.secs2().get_ascii().unwrap(), "HI");
        assert!(send.join().unwrap().is_none());

        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_wbit_reply_t3() {
        let (a, b) = loopback_pair();
        let ca = Secs1Circuit::start(a, cfg_fast()).unwrap();
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let primary = msg(0x41, true, "Q");
        let send = thread::spawn({
            let ca = Arc::clone(&ca);
            let p = primary.clone();
            move || ca.send(p).unwrap()
        });

        let got = cb.recv_primary().unwrap();
        assert!(got.wbit());
        let rsp = reply_to(&got, "A");
        cb.send(rsp).unwrap();

        let reply = send.join().unwrap().expect("reply");
        assert_eq!(reply.secs2().get_ascii().unwrap(), "A");
        assert_eq!(reply.get_function(), 2);

        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_t3_timeout() {
        let (a, b) = loopback_pair();
        let mut cfg = cfg_fast();
        cfg.t3 = Duration::from_millis(150);
        let ca = Secs1Circuit::start(a, cfg).unwrap();
        // Peer circuit accepts but never replies.
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let primary = msg(0x51, true, "T");
        let err = ca.send(primary).unwrap_err();
        assert_eq!(err, Error::TimeoutT3);

        // Drain primary so peer doesn't hang forever.
        let _ = cb.poll_primary(Duration::from_millis(200));
        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_multi_block_primary() {
        let (a, b) = loopback_pair();
        let ca = Secs1Circuit::start(a, cfg_fast()).unwrap();
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let body = "Z".repeat(500);
        let header = [0x00, 0x0A, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x61];
        let m = Secs1Message::build_data_message(&header, Secs2::ascii(&body).unwrap()).unwrap();
        assert!(m.to_blocks().len() >= 2);

        let send = thread::spawn({
            let ca = Arc::clone(&ca);
            move || ca.send(m).unwrap()
        });

        let got = cb.recv_primary().unwrap();
        assert_eq!(got.secs2().get_ascii().unwrap(), body);
        assert!(send.join().unwrap().is_none());

        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_drops_mismatched_device_id_block() {
        // Receiver checks Device-ID=10; sender uses Device-ID=20 → ACK but no primary.
        let (a, b) = loopback_pair();
        let mut rx_cfg = cfg_fast();
        rx_cfg.device_id = 10;
        rx_cfg.check_block_device_id = true;
        let ca = Secs1Circuit::start(a, cfg_fast()).unwrap();
        let cb = Secs1Circuit::start(b, rx_cfg).unwrap();

        let m = msg_device(20, 0x71, "NOPE");
        assert_eq!(m.device_id(), 20);
        ca.send(m).unwrap();

        let none = cb.poll_primary(Duration::from_millis(300)).unwrap();
        assert!(none.is_none(), "mismatched Device-ID must not deliver primary");

        // Matching Device-ID still works.
        let ok = msg_device(10, 0x72, "YES");
        let send = thread::spawn({
            let ca = Arc::clone(&ca);
            move || ca.send(ok).unwrap()
        });
        let got = cb.recv_primary().unwrap();
        assert_eq!(got.secs2().get_ascii().unwrap(), "YES");
        send.join().unwrap();

        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_device_id_check_disabled_accepts_any() {
        let (a, b) = loopback_pair();
        let mut rx_cfg = cfg_fast();
        rx_cfg.device_id = 10;
        rx_cfg.check_block_device_id = false;
        let ca = Secs1Circuit::start(a, cfg_fast()).unwrap();
        let cb = Secs1Circuit::start(b, rx_cfg).unwrap();

        let m = msg_device(99, 0x73, "ANY");
        let send = thread::spawn({
            let ca = Arc::clone(&ca);
            move || ca.send(m).unwrap()
        });
        let got = cb.recv_primary().unwrap();
        assert_eq!(got.device_id(), 99);
        assert_eq!(got.secs2().get_ascii().unwrap(), "ANY");
        send.join().unwrap();

        ca.close();
        cb.close();
    }

    #[test]
    fn circuit_close_unblocks_pending_wbit_send() {
        // Peer never replies; close must fail waiter with ChannelShutdown (no hang).
        let (a, b) = loopback_pair();
        let mut cfg = cfg_fast();
        cfg.t3 = Duration::from_secs(30);
        let ca = Secs1Circuit::start(a, cfg).unwrap();
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let primary = msg(0x81, true, "HANG");
        let ca2 = Arc::clone(&ca);
        let send = thread::spawn(move || ca2.send(primary));

        // Ensure primary is queued/on wire, then close sender circuit.
        let _ = cb.poll_primary(Duration::from_millis(500));
        thread::sleep(Duration::from_millis(50));
        ca.close();

        let err = send
            .join()
            .expect("join")
            .expect_err("must not hang / must error");
        assert!(
            matches!(err, Error::ChannelShutdown | Error::TimeoutT3),
            "unexpected {err:?}"
        );
        cb.close();
    }

    #[test]
    fn circuit_peer_drop_unblocks_pending_wbit_send() {
        // Peer TCP close (not local close) must fail T3 waiter promptly.
        let (a, b) = loopback_pair();
        let mut cfg = cfg_fast();
        cfg.t3 = Duration::from_secs(30);
        let ca = Secs1Circuit::start(a, cfg).unwrap();
        let cb = Secs1Circuit::start(b, cfg_fast()).unwrap();

        let primary = msg(0x81, true, "PEER");
        let ca2 = Arc::clone(&ca);
        let send = thread::spawn(move || ca2.send(primary));

        let _ = cb.poll_primary(Duration::from_millis(500));
        thread::sleep(Duration::from_millis(50));
        // Drop peer circuit → TCP RST/FIN on ca's socket.
        cb.close();

        let err = send
            .join()
            .expect("join")
            .expect_err("peer drop must not hang");
        assert!(
            matches!(err, Error::ChannelShutdown | Error::TimeoutT3 | Error::RetryOver),
            "unexpected {err:?}"
        );
        ca.close();
    }
}
