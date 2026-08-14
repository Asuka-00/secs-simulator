//! HSMS-SS (SEMI-E37.1) communicator: config, Select, long-lived selected session.
//!
//! Batch6 oracle: factory NewInstance, session-id validation, timeout/linktest.
//! Long-lived open: keep `HsmsSessionIo`, selected primary loop, send DATA/LINKTEST.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::hsms::{
    active_select, build_data_message, build_data_reply, build_linktest_request,
    build_separate_request, passive_await_select_req, passive_select,
    passive_select_already_used, reply_select_status, Error as HsmsError, HsmsCommunicateState,
    HsmsConnectionMode, HsmsMessage, HsmsMessageType, HsmsPassThrough, HsmsSessionIo,
    HsmsTcpChannel, SelectedDispatch, SelectStatus, SystemBytesCounter,
};
use crate::open_close::{OpenAndCloseable, OpenCloseError, OpenCloseState};
use crate::property::{
    BooleanProperty, IntegerProperty, ListenerId, ObjectProperty, TimeoutAndUnit, TimeoutProperty,
};
use crate::secs2::Secs2;
use crate::timeout::SecsTimeout;

type DataRecvFn = Arc<dyn Fn(&HsmsMessage) + Send + Sync + 'static>;

/// Session-ID out of range (`SessionIdIllegalArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionIdIllegalArgument;

impl std::fmt::Display for SessionIdIllegalArgument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal session id")
    }
}

impl std::error::Error for SessionIdIllegalArgument {}

/// Placeholder GEM accessor (non-null for oracle).
#[derive(Debug, Clone, Default)]
pub struct GemHandle;

/// HSMS-SS configuration.
pub struct HsmsSsCommunicatorConfig {
    session_id: IntegerProperty,
    connection_mode: ObjectProperty<HsmsConnectionMode>,
    socket_addr: Mutex<Option<SocketAddr>>,
    timeout: SecsTimeout,
    linktest_time: TimeoutProperty,
    do_linktest: BooleanProperty,
    /// Equipment role → system-bytes high 2 = session id (`IsEquip`).
    is_equip: BooleanProperty,
    /// Re-bind listen socket after passive session ends (`DoRebindIfPassive`, default true).
    do_rebind_if_passive: BooleanProperty,
    /// Configured rebind interval property (C# default 10s; Open path uses T5 between rebinds).
    rebind_if_passive_time: TimeoutProperty,
    gem: GemHandle,
}

impl Default for HsmsSsCommunicatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl HsmsSsCommunicatorConfig {
    pub fn new() -> Self {
        Self {
            session_id: IntegerProperty::new(10),
            connection_mode: ObjectProperty::new(HsmsConnectionMode::Passive),
            socket_addr: Mutex::new(None),
            timeout: SecsTimeout::new(),
            linktest_time: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(120.0)),
            do_linktest: BooleanProperty::new(false),
            is_equip: BooleanProperty::new(false),
            do_rebind_if_passive: BooleanProperty::new(true),
            rebind_if_passive_time: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(10.0)),
            gem: GemHandle,
        }
    }

    pub fn set_session_id(&self, id: i32) -> Result<(), SessionIdIllegalArgument> {
        if !(0..=0x7FFF).contains(&id) {
            return Err(SessionIdIllegalArgument);
        }
        self.session_id.set(id);
        Ok(())
    }

    pub fn session_id(&self) -> i32 {
        self.session_id.int_value()
    }

    pub fn session_id_prop(&self) -> &IntegerProperty {
        &self.session_id
    }

    pub fn set_connection_mode(&self, mode: HsmsConnectionMode) {
        self.connection_mode.set(mode);
    }

    pub fn connection_mode(&self) -> HsmsConnectionMode {
        self.connection_mode.get()
    }

    pub fn set_socket_address(&self, addr: SocketAddr) {
        *self.socket_addr.lock().expect("socket addr") = Some(addr);
    }

    pub fn socket_address(&self) -> Option<SocketAddr> {
        *self.socket_addr.lock().expect("socket addr")
    }

    pub fn timeout(&self) -> &SecsTimeout {
        &self.timeout
    }

    pub fn linktest(&self, seconds: f32) {
        self.linktest_time.set_seconds_f32(seconds);
        self.do_linktest.set_true();
    }

    pub fn not_linktest(&self) {
        self.do_linktest.set_false();
    }

    pub fn linktest_time(&self) -> &TimeoutProperty {
        &self.linktest_time
    }

    pub fn do_linktest(&self) -> &BooleanProperty {
        &self.do_linktest
    }

    /// `IsEquip(bool)` — equipment vs host role for system-bytes.
    pub fn set_is_equip(&self, equip: bool) {
        self.is_equip.set(equip);
    }

    pub fn is_equip(&self) -> bool {
        self.is_equip.boolean_value()
    }

    pub fn is_equip_prop(&self) -> &BooleanProperty {
        &self.is_equip
    }

    /// Enable rebind-if-passive with interval seconds (stores property; Open uses T5 between rebinds).
    pub fn rebind_if_passive(&self, seconds: f32) {
        self.rebind_if_passive_time.set_seconds_f32(seconds);
        self.do_rebind_if_passive.set_true();
    }

    /// Disable rebind-if-passive.
    pub fn not_rebind_if_passive(&self) {
        self.do_rebind_if_passive.set_false();
    }

    pub fn do_rebind_if_passive(&self) -> &BooleanProperty {
        &self.do_rebind_if_passive
    }

    pub fn rebind_if_passive_time(&self) -> &TimeoutProperty {
        &self.rebind_if_passive_time
    }

    pub fn gem(&self) -> &GemHandle {
        &self.gem
    }
}

/// HSMS-SS communicator (construction + Select + long-lived selected session).
pub struct HsmsSsCommunicator {
    config: HsmsSsCommunicatorConfig,
    mode: HsmsConnectionMode,
    open: AtomicBool,
    /// Opened/closed flags (`OpenAndCloseable` / AbstractBaseCommunicator).
    lifecycle: OpenCloseState,
    gem: GemHandle,
    state: ObjectProperty<HsmsCommunicateState>,
    sys_bytes: SystemBytesCounter,
    /// Session channel claim (`SetChannel` / `UnsetChannel`).
    channel_held: AtomicBool,
    /// Selected SessionIo (send path).
    io: Mutex<Option<Arc<HsmsSessionIo>>>,
    /// DATA primaries from selected loop (recv without holding `io` lock).
    data_rx: Mutex<Option<Receiver<HsmsMessage>>>,
    loop_handle: Mutex<Option<JoinHandle<()>>>,
    /// Periodic linktest task handle (when `DoLinktest`).
    linktest_handle: Mutex<Option<JoinHandle<()>>>,
    /// Stop flag for linktest loop (set on teardown).
    linktest_stop: Arc<AtomicBool>,
    /// Count of successful auto-linktests (test/observability).
    linktest_ok_count: Arc<AtomicU64>,
    /// DATA receive listeners (`HsmsMessageReceiveListener`).
    data_listeners: Arc<Mutex<Vec<(ListenerId, DataRecvFn)>>>,
    next_listener_id: AtomicU64,
    /// Background Open worker (T5 retry / accept loop).
    open_handle: Mutex<Option<JoinHandle<()>>>,
    /// Stop background Open (set by `close`).
    open_stop: Arc<AtomicBool>,
    /// Successful passive bind+select cycles (rebind observability).
    passive_session_count: Arc<AtomicU64>,
    /// Concurrent accept workers (`open_passive_listen`); reaped on close.
    accept_workers: Mutex<Vec<JoinHandle<()>>>,
    /// HSMS message pass-through (try-send / sended / receive); lives across sessions.
    pass_through: Arc<HsmsPassThrough>,
}

impl HsmsSsCommunicator {
    /// `HsmsSsCommunicator.NewInstance(config)` — Active/Passive branch, no Open.
    pub fn new_instance(config: HsmsSsCommunicatorConfig) -> Self {
        let mode = config.connection_mode();
        let gem = GemHandle;
        Self {
            config,
            mode,
            open: AtomicBool::new(false),
            lifecycle: OpenCloseState::new(),
            gem,
            state: ObjectProperty::new(HsmsCommunicateState::NotConnected),
            sys_bytes: SystemBytesCounter::new(),
            channel_held: AtomicBool::new(false),
            io: Mutex::new(None),
            data_rx: Mutex::new(None),
            loop_handle: Mutex::new(None),
            linktest_handle: Mutex::new(None),
            linktest_stop: Arc::new(AtomicBool::new(true)),
            linktest_ok_count: Arc::new(AtomicU64::new(0)),
            data_listeners: Arc::new(Mutex::new(Vec::new())),
            next_listener_id: AtomicU64::new(1),
            open_handle: Mutex::new(None),
            open_stop: Arc::new(AtomicBool::new(true)),
            passive_session_count: Arc::new(AtomicU64::new(0)),
            accept_workers: Mutex::new(Vec::new()),
            pass_through: Arc::new(HsmsPassThrough::new()),
        }
    }

    /// Shared pass-through facade (`HsmsMessagePassThroughObservable`).
    pub fn pass_through(&self) -> &HsmsPassThrough {
        &self.pass_through
    }

    /// `AddTrySendHsmsMessagePassThroughListener`.
    pub fn add_try_send_pass_through<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        self.pass_through.add_try_send(f)
    }

    /// `AddSendedHsmsMessagePassThroughListener`.
    pub fn add_sended_pass_through<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        self.pass_through.add_sended(f)
    }

    /// `AddReceiveHsmsMessagePassThroughListener`.
    pub fn add_receive_pass_through<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        self.pass_through.add_receive(f)
    }

    pub fn is_open(&self) -> bool {
        self.lifecycle.is_open() || self.open.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.lifecycle.is_closed()
    }

    /// Try claim session channel (`SetChannel`); false if already held.
    fn try_set_channel(&self) -> bool {
        self.channel_held
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn unset_channel(&self) {
        self.channel_held.store(false, Ordering::SeqCst);
    }

    pub fn is_equip(&self) -> bool {
        self.config.is_equip()
    }

    pub fn session_id(&self) -> i32 {
        self.config.session_id()
    }

    /// Next system-bytes (host zeros / equip session-id high 2).
    fn next_sys_bytes(&self) -> [u8; 4] {
        self.sys_bytes
            .next(self.config.is_equip(), self.session_id())
    }

    /// Successful periodic linktest count (resets on each live session install).
    pub fn linktest_ok_count(&self) -> u64 {
        self.linktest_ok_count.load(Ordering::SeqCst)
    }

    /// How many passive accept+select cycles completed (includes rebinds).
    pub fn passive_session_count(&self) -> u64 {
        self.passive_session_count.load(Ordering::SeqCst)
    }

    pub fn connection_mode(&self) -> HsmsConnectionMode {
        self.mode
    }

    pub fn gem(&self) -> &GemHandle {
        &self.gem
    }

    pub fn config(&self) -> &HsmsSsCommunicatorConfig {
        &self.config
    }

    /// Current HSMS communicate state (NOT_CONNECTED / NOT_SELECTED / SELECTED).
    pub fn hsms_communicate_state(&self) -> HsmsCommunicateState {
        self.state.get()
    }

    pub fn communicate_state_prop(&self) -> &ObjectProperty<HsmsCommunicateState> {
        &self.state
    }

    /// Wait until state equals `expected` (protocol path).
    pub fn wait_until_hsms_communicate_state(&self, expected: HsmsCommunicateState) {
        self.state.wait_until_equal_to(&expected);
    }

    /// `AddHsmsMessageReceiveListener` — called for each DATA primary.
    pub fn add_hsms_message_receive_listener<F>(&self, f: F) -> ListenerId
    where
        F: Fn(&HsmsMessage) + Send + Sync + 'static,
    {
        let id = ListenerId::from_raw(self.next_listener_id.fetch_add(1, Ordering::SeqCst));
        self.data_listeners
            .lock()
            .expect("data listeners")
            .push((id, Arc::new(f)));
        id
    }

    /// `RemoveHsmsMessageReceiveListener`.
    pub fn remove_hsms_message_receive_listener(&self, id: ListenerId) -> bool {
        let mut g = self.data_listeners.lock().expect("data listeners");
        let before = g.len();
        g.retain(|(i, _)| *i != id);
        g.len() < before
    }

    /// Active: connect → SELECT → keep SessionIo + selected dispatch loop.
    ///
    /// One connection attempt (no T5 retry). Prefer [`open_active_with_t5_retry`] for full Open.
    pub fn open_active(&self) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Active {
            return Err(HsmsError::Protocol("connection mode is not ACTIVE"));
        }
        let addr = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let stream = TcpStream::connect(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);
        self.state.set(HsmsCommunicateState::NotSelected);

        // Select on a clone so the original stream can become SessionIo.
        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let sys = self.next_sys_bytes();
        let t6 = self.config.timeout().t6().get().as_duration();
        let ok = active_select(&mut ch, sys, t6)?;
        drop(ch);
        if !ok {
            self.state.set(HsmsCommunicateState::NotSelected);
            return Err(HsmsError::Protocol("SELECT.rsp not success/actived"));
        }

        self.install_live_session(stream, true)
    }

    /// Full Active Open: background worker retries connect+select with T5 between attempts.
    ///
    /// Call on `Arc<HsmsSsCommunicator>`. Stops when [`close`] is called.
    /// After a session ends (peer close / SEPARATE), waits T5 and reconnects.
    pub fn open_active_with_t5_retry(self: &Arc<Self>) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Active {
            return Err(HsmsError::Protocol("connection mode is not ACTIVE"));
        }
        let _ = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        self.open_stop.store(false, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);

        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            while !this.open_stop.load(Ordering::SeqCst) {
                match this.open_active() {
                    Ok(()) => {
                        // Hold until leave SELECTED (peer drop / SEPARATE / linktest fail).
                        while !this.open_stop.load(Ordering::SeqCst)
                            && this.hsms_communicate_state() == HsmsCommunicateState::Selected
                        {
                            thread::sleep(Duration::from_millis(20));
                        }
                        // Clean session shells; leave open=true for retry.
                        this.teardown_live_io_only();
                        if this.hsms_communicate_state() == HsmsCommunicateState::Selected {
                            this.state.set(HsmsCommunicateState::NotConnected);
                        }
                    }
                    Err(_) => {
                        // Connect/select failed — T5 then retry.
                    }
                }
                if this.open_stop.load(Ordering::SeqCst) {
                    break;
                }
                let t5 = this.config.timeout().t5().get().as_duration();
                sleep_chunked(t5, &this.open_stop);
            }
        });
        *self.open_handle.lock().expect("open handle") = Some(handle);
        Ok(())
    }

    /// Passive: bind/accept one → SELECT (T7) → SessionIo + selected loop.
    ///
    /// After bind, `is_open()` is true so Active may connect.
    pub fn open_passive(&self) -> Result<(), HsmsError> {
        self.open_passive_inner(false)
    }

    /// `interruptible_accept`: poll accept and honor `open_stop` (background rebind path).
    fn open_passive_inner(&self, interruptible_accept: bool) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let addr = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let listener = TcpListener::bind(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);

        let stream = if interruptible_accept {
            listener
                .set_nonblocking(true)
                .map_err(HsmsError::from)?;
            let s = accept_until(&listener, &self.open_stop)?;
            s.set_nonblocking(false).map_err(HsmsError::from)?;
            s
        } else {
            let (s, _) = listener.accept().map_err(HsmsError::from)?;
            s
        };
        self.state.set(HsmsCommunicateState::NotSelected);

        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let t7 = self.config.timeout().t7().get().as_duration();
        passive_select(&mut ch, t7)?;
        drop(ch);

        if !self.try_set_channel() {
            // Should not happen on one-shot path; reject if racing.
            return Err(HsmsError::Protocol("session channel already set"));
        }
        if let Err(e) = self.install_live_session(stream, false) {
            self.unset_channel();
            return Err(e);
        }
        self.passive_session_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Passive listen loop: one bind, accept forever; extra SELECT → ALREADY_USED.
    ///
    /// Parity with `OpenPassive` accept loop + `SetChannel` failure → ALREADY_USED.
    /// Stops on [`close`]. Bind failure is returned (does not leave a silent open).
    pub fn open_passive_listen(self: &Arc<Self>) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let addr = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        self.lifecycle.mark_open().map_err(|e| match e {
            OpenCloseError::AlreadyOpened => HsmsError::Protocol("already opened"),
            OpenCloseError::AlreadyClosed => HsmsError::Protocol("already closed"),
            OpenCloseError::Failed => HsmsError::Protocol("open failed"),
        })?;
        self.open_stop.store(false, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);
        self.passive_session_count.store(0, Ordering::SeqCst);

        let (bind_tx, bind_rx) = mpsc::channel();
        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            let listener = match TcpListener::bind(addr) {
                Ok(l) => {
                    let _ = bind_tx.send(Ok(()));
                    l
                }
                Err(e) => {
                    let _ = bind_tx.send(Err(e));
                    return;
                }
            };
            let _ = listener.set_nonblocking(true);
            let t7 = this.config.timeout().t7().get().as_duration();

            while !this.open_stop.load(Ordering::SeqCst) {
                let stream = match accept_until(&listener, &this.open_stop) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let _ = stream.set_nonblocking(false);

                // Concurrent accept while selected: track workers for close join.
                let this2 = Arc::clone(&this);
                let h = thread::spawn(move || {
                    this2.handle_passive_accepted(stream, t7);
                });
                this.accept_workers.lock().expect("accept workers").push(h);
            }
        });

        match bind_rx.recv() {
            Ok(Ok(())) => {
                *self.open_handle.lock().expect("open handle") = Some(handle);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                self.open_stop.store(true, Ordering::SeqCst);
                self.open.store(false, Ordering::SeqCst);
                let _ = self.lifecycle.mark_closed();
                Err(HsmsError::from(e))
            }
            Err(_) => {
                let _ = handle.join();
                self.open_stop.store(true, Ordering::SeqCst);
                self.open.store(false, Ordering::SeqCst);
                let _ = self.lifecycle.mark_closed();
                Err(HsmsError::Protocol("passive listen bind thread died"))
            }
        }
    }

    /// Handle one accepted passive connection (SELECT → SUCCESS or ALREADY_USED).
    fn handle_passive_accepted(&self, stream: TcpStream, t7: Duration) {
        let mut ch = HsmsTcpChannel::new(stream);
        let initiate = match passive_await_select_req(&mut ch, t7) {
            Ok(m) => m,
            Err(_) => return,
        };

        if !self.try_set_channel() {
            let _ = reply_select_status(&mut ch, &initiate, SelectStatus::AlreadyUsed);
            return;
        }

        if reply_select_status(&mut ch, &initiate, SelectStatus::Success).is_err() {
            self.unset_channel();
            return;
        }

        let stream = ch.into_inner();
        if self.install_live_session(stream, false).is_err() {
            self.unset_channel();
            return;
        }
        self.passive_session_count.fetch_add(1, Ordering::SeqCst);

        // Stay attached until selected session ends.
        while !self.open_stop.load(Ordering::SeqCst)
            && self.hsms_communicate_state() == HsmsCommunicateState::Selected
        {
            thread::sleep(Duration::from_millis(20));
        }
        self.teardown_live_io_only();
        self.unset_channel();
        if self.hsms_communicate_state() != HsmsCommunicateState::NotConnected {
            self.state.set(HsmsCommunicateState::NotConnected);
        }
    }

    /// Full Passive Open: background worker re-binds after session ends when `DoRebindIfPassive`.
    ///
    /// Between rebinds waits **T5** (parity with C# `Open` loop). Stops on [`close`]
    /// or when rebind is disabled after the first session.
    pub fn open_passive_with_rebind(self: &Arc<Self>) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let _ = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        self.open_stop.store(false, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);
        self.passive_session_count.store(0, Ordering::SeqCst);

        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            loop {
                if this.open_stop.load(Ordering::SeqCst) {
                    break;
                }
                match this.open_passive_inner(true) {
                    Ok(()) => {
                        while !this.open_stop.load(Ordering::SeqCst)
                            && this.hsms_communicate_state() == HsmsCommunicateState::Selected
                        {
                            thread::sleep(Duration::from_millis(20));
                        }
                        this.teardown_live_io_only();
                        this.unset_channel();
                        if this.hsms_communicate_state() == HsmsCommunicateState::Selected
                            || this.hsms_communicate_state()
                                == HsmsCommunicateState::NotSelected
                        {
                            this.state
                                .set(HsmsCommunicateState::NotConnected);
                        }
                    }
                    Err(_) => {
                        // Bind/accept/select failed or stop during accept.
                        this.unset_channel();
                    }
                }
                if this.open_stop.load(Ordering::SeqCst) {
                    break;
                }
                if !this.config.do_rebind_if_passive().boolean_value() {
                    break;
                }
                // C# Open: T5.Sleep between OpenPassive rebinds.
                let t5 = this.config.timeout().t5().get().as_duration();
                sleep_chunked(t5, &this.open_stop);
            }
        });
        *self.open_handle.lock().expect("open handle") = Some(handle);
        Ok(())
    }

    /// One-shot Active select then drop channel (legacy smoke; prefer [`open_active`]).
    pub fn open_active_select_once(&self) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Active {
            return Err(HsmsError::Protocol("connection mode is not ACTIVE"));
        }
        let addr = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let stream = TcpStream::connect(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);
        self.state.set(HsmsCommunicateState::NotSelected);

        let mut ch = HsmsTcpChannel::new(stream);
        let sys = self.next_sys_bytes();
        let t6 = self.config.timeout().t6().get().as_duration();
        let ok = active_select(&mut ch, sys, t6)?;
        if !ok {
            self.state.set(HsmsCommunicateState::NotSelected);
            return Err(HsmsError::Protocol("SELECT.rsp not success/actived"));
        }
        self.state.set(HsmsCommunicateState::Selected);
        Ok(())
    }

    /// One-shot Passive select then drop channel (legacy smoke; prefer [`open_passive`]).
    pub fn open_passive_select_once(&self) -> Result<(), HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let addr = self
            .config
            .socket_address()
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let listener = TcpListener::bind(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);

        let (stream, _) = listener.accept().map_err(HsmsError::from)?;
        self.state.set(HsmsCommunicateState::NotSelected);

        let mut ch = HsmsTcpChannel::new(stream);
        let t7 = self.config.timeout().t7().get().as_duration();
        passive_select(&mut ch, t7)?;
        self.state.set(HsmsCommunicateState::Selected);
        Ok(())
    }

    fn install_live_session(&self, stream: TcpStream, is_active: bool) -> Result<(), HsmsError> {
        // Tear down previous session first.
        self.teardown_live_io_only();

        let t8 = self.config.timeout().t8().get().as_duration();
        let io = Arc::new(HsmsSessionIo::with_pass_through_t8(
            stream,
            Arc::clone(&self.pass_through),
            t8,
        )?);
        let (data_tx, data_rx) = mpsc::channel();
        let io_loop = Arc::clone(&io);
        let state = self.state.clone();
        let listeners = Arc::clone(&self.data_listeners);
        let handle = thread::spawn(move || {
            selected_main_loop(io_loop, is_active, data_tx, state, listeners);
        });

        *self.io.lock().expect("io") = Some(Arc::clone(&io));
        *self.data_rx.lock().expect("data_rx") = Some(data_rx);
        *self.loop_handle.lock().expect("loop") = Some(handle);
        self.state.set(HsmsCommunicateState::Selected);

        // Periodic linktest task (SEMI idle linktest; simplified interval loop).
        self.linktest_stop.store(false, Ordering::SeqCst);
        self.linktest_ok_count.store(0, Ordering::SeqCst);
        let stop = Arc::clone(&self.linktest_stop);
        let ok_cnt = Arc::clone(&self.linktest_ok_count);
        let do_lt = self.config.do_linktest().clone();
        let lt_time = self.config.linktest_time().clone();
        let t6 = self.config.timeout().t6().clone();
        let sys = SystemBytesCounter::new(); // local serial for auto linktests
        let is_equip = self.config.is_equip();
        let sid = self.session_id();
        let state_lt = self.state.clone();
        let lt_handle = thread::spawn(move || {
            linktest_loop(io, stop, ok_cnt, do_lt, lt_time, t6, sys, is_equip, sid, state_lt);
        });
        *self.linktest_handle.lock().expect("linktest") = Some(lt_handle);
        Ok(())
    }

    /// Snapshot session Arc under lock, then run `f` **without** holding the mutex
    /// (send/reply wait may block for T3/T6 — must not stall `close`/teardown).
    fn with_io<R>(
        &self,
        f: impl FnOnce(&HsmsSessionIo) -> Result<R, HsmsError>,
    ) -> Result<R, HsmsError> {
        let io = {
            let g = self.io.lock().expect("io");
            g.as_ref()
                .cloned()
                .ok_or(HsmsError::Protocol("not selected / no session"))?
        };
        f(&io)
    }

    /// Send an already-built HSMS message (reply wait uses T3/T6 from config).
    ///
    /// Parity: `WaitUntilSended` always uses T3 (data) / T6 (control); reply wait
    /// only applies when a reply is expected (same timeouts).
    pub fn send(&self, msg: &HsmsMessage) -> Result<Option<HsmsMessage>, HsmsError> {
        let timeout = match HsmsSessionIo::reply_timeout_class(msg) {
            Some(crate::hsms::ReplyTimeoutClass::T3) => {
                self.config.timeout().t3().get().as_duration()
            }
            Some(crate::hsms::ReplyTimeoutClass::T6) => {
                self.config.timeout().t6().get().as_duration()
            }
            // No reply: still bound WaitUntilSended (C#: data→T3, control→T6).
            None if msg.is_data_message() => self.config.timeout().t3().get().as_duration(),
            None => self.config.timeout().t6().get().as_duration(),
        };
        self.with_io(|io| io.send(msg, timeout))
    }

    /// Build + send primary DATA (`Session.Send(strm, func, wbit, secs2)`).
    pub fn send_data(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        let sys = self.next_sys_bytes();
        let msg = build_data_message(self.session_id(), strm, func, wbit, body, sys)?;
        self.send(&msg)
    }

    /// Reply to a primary DATA (reuses system-bytes).
    pub fn send_data_reply(
        &self,
        primary: &HsmsMessage,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<(), HsmsError> {
        let msg = build_data_reply(self.session_id(), primary, strm, func, wbit, body)?;
        self.send(&msg)?;
        Ok(())
    }

    /// Reply using only primary header-10-bytes (entity SxF0 path).
    pub fn send_data_reply_from_header(
        &self,
        primary_header: &[u8; 10],
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<(), HsmsError> {
        let msg = crate::hsms::build_data_reply_from_header(
            self.session_id(),
            primary_header,
            strm,
            func,
            wbit,
            body,
        )?;
        self.send(&msg)?;
        Ok(())
    }

    /// GEM S9Fx primary (`AbstractGem.S9fx`): stream=9, wbit=false, Binary(MHEAD).
    pub fn send_s9fx(
        &self,
        func: crate::gem::S9Func,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        let sys = self.next_sys_bytes();
        let msg = crate::gem::build_s9_message(self.session_id(), func, ref_msg, sys)?;
        self.send(&msg)
    }

    pub fn send_s9f1(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::UnrecognizedDeviceId, ref_msg)
    }

    pub fn send_s9f3(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::UnrecognizedStream, ref_msg)
    }

    pub fn send_s9f5(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::UnrecognizedFunction, ref_msg)
    }

    pub fn send_s9f7(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::IllegalData, ref_msg)
    }

    pub fn send_s9f9(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::TransactionTimeout, ref_msg)
    }

    pub fn send_s9f11(
        &self,
        ref_msg: &dyn crate::SecsMessage,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        self.send_s9fx(crate::gem::S9Func::DataTooLong, ref_msg)
    }

    /// Send LINKTEST.req and wait LINKTEST.rsp (T6).
    pub fn send_linktest(&self) -> Result<bool, HsmsError> {
        let sys = self.next_sys_bytes();
        let req = build_linktest_request(sys)?;
        match self.send(&req)? {
            Some(rsp) if rsp.message_type() == HsmsMessageType::LinktestRsp => Ok(true),
            Some(_) => Ok(false),
            None => Ok(false),
        }
    }

    /// Take next received DATA primary (blocks).
    pub fn take_data_message(&self) -> Result<HsmsMessage, HsmsError> {
        let g = self.data_rx.lock().expect("data_rx");
        let rx = g
            .as_ref()
            .ok_or(HsmsError::Protocol("not selected / no session"))?;
        rx.recv().map_err(|_| HsmsError::DetectTerminate)
    }

    /// Poll next received DATA primary.
    pub fn poll_data_message(&self, timeout: Duration) -> Result<Option<HsmsMessage>, HsmsError> {
        let g = self.data_rx.lock().expect("data_rx");
        let rx = g
            .as_ref()
            .ok_or(HsmsError::Protocol("not selected / no session"))?;
        match rx.recv_timeout(timeout) {
            Ok(m) => Ok(Some(m)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(HsmsError::DetectTerminate),
        }
    }

    /// Send SEPARATE.req (no reply) and tear down session I/O.
    pub fn separate(&self) -> Result<(), HsmsError> {
        let sys = self.next_sys_bytes();
        let msg = build_separate_request(sys)?;
        let _ = self.with_io(|io| io.send_noreply(&msg));
        self.teardown_live(HsmsCommunicateState::NotConnected);
        Ok(())
    }

    /// Shutdown session and mark not connected.
    pub fn close(&self) {
        self.open_stop.store(true, Ordering::SeqCst);
        self.teardown_live(HsmsCommunicateState::NotConnected);
        self.unset_channel();
        self.open.store(false, Ordering::SeqCst);
        self.lifecycle.mark_closed();
        if let Some(h) = self.open_handle.lock().expect("open handle").take() {
            let _ = h.join();
        }
        let workers: Vec<_> = self
            .accept_workers
            .lock()
            .expect("accept workers")
            .drain(..)
            .collect();
        for h in workers {
            let _ = h.join();
        }
    }

    fn teardown_live_io_only(&self) {
        self.linktest_stop.store(true, Ordering::SeqCst);
        if let Some(io) = self.io.lock().expect("io").take() {
            io.shutdown();
        }
        // Join selected loop first so data_tx is dropped and blocked recv wakes.
        if let Some(h) = self.loop_handle.lock().expect("loop").take() {
            let _ = h.join();
        }
        if let Some(h) = self.linktest_handle.lock().expect("linktest").take() {
            let _ = h.join();
        }
        *self.data_rx.lock().expect("data_rx") = None;
    }

    fn teardown_live(&self, state: HsmsCommunicateState) {
        self.teardown_live_io_only();
        self.state.set(state);
    }
}

/// Selected MainTask: dispatch primaries; DATA → listeners + app queue; SEPARATE → exit.
fn selected_main_loop(
    io: Arc<HsmsSessionIo>,
    is_active: bool,
    data_tx: Sender<HsmsMessage>,
    state: ObjectProperty<HsmsCommunicateState>,
    listeners: Arc<Mutex<Vec<(ListenerId, DataRecvFn)>>>,
) {
    loop {
        let msg = match io.take_primary() {
            Ok(m) => m,
            Err(_) => {
                state.set(HsmsCommunicateState::NotSelected);
                break;
            }
        };
        let dispatch = if is_active {
            io.dispatch_active_primary(&msg)
        } else {
            io.dispatch_passive_primary(&msg)
        };
        match dispatch {
            Ok(SelectedDispatch::Data(m)) => {
                // Snapshot so a listener can send/reply without holding this lock
                // (send waits WaitUntilSended; holding the lock deadlocks add/remove).
                let snap: Vec<DataRecvFn> = {
                    let ls = listeners.lock().expect("data listeners");
                    ls.iter().map(|(_, f)| Arc::clone(f)).collect()
                };
                for f in snap {
                    f(&m);
                }
                if data_tx.send(m).is_err() {
                    break;
                }
            }
            Ok(SelectedDispatch::Continue) => {}
            Ok(SelectedDispatch::Separate) => {
                state.set(HsmsCommunicateState::NotSelected);
                break;
            }
            Err(_) => {
                state.set(HsmsCommunicateState::NotSelected);
                break;
            }
        }
    }
}

/// Sleep `total` in small chunks, aborting early if `stop` is set.
fn sleep_chunked(total: Duration, stop: &AtomicBool) {
    let chunk = Duration::from_millis(20);
    let mut waited = Duration::ZERO;
    while waited < total {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let step = if total - waited < chunk {
            total - waited
        } else {
            chunk
        };
        thread::sleep(step);
        waited += step;
    }
}

/// Non-blocking accept loop; returns `DetectTerminate` when `stop` is set.
fn accept_until(listener: &TcpListener, stop: &AtomicBool) -> Result<TcpStream, HsmsError> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(HsmsError::DetectTerminate);
        }
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(HsmsError::from(e)),
        }
    }
}

/// Periodic LINKTEST while selected (`TaskLinktest` simplified).
///
/// When `DoLinktest` is true, wait `LinktestTime` then issue LINKTEST.req.
/// Failure → shutdown channel and leave SELECTED (parity: channel close).
/// Config change / stop ends the wait early via chunked sleep.
fn linktest_loop(
    io: Arc<HsmsSessionIo>,
    stop: Arc<AtomicBool>,
    ok_cnt: Arc<std::sync::atomic::AtomicU64>,
    do_lt: BooleanProperty,
    lt_time: TimeoutProperty,
    t6: TimeoutProperty,
    sys: SystemBytesCounter,
    is_equip: bool,
    sid: i32,
    state: ObjectProperty<HsmsCommunicateState>,
) {
    let activity = Arc::clone(io.activity());
    while !stop.load(Ordering::SeqCst) {
        if state.get() != HsmsCommunicateState::Selected {
            break;
        }
        if !do_lt.boolean_value() {
            // Disabled: sleep until enabled / stop (still honor activity wake).
            let _ = activity.wait_idle(Duration::from_millis(50), &stop);
            continue;
        }

        // Idle interval: I/O activity restarts the timer (LinktestReset).
        let interval = lt_time.get().as_duration();
        let idle_done = activity.wait_idle(interval, &stop);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if !idle_done {
            // Activity or config churn → restart wait without sending.
            continue;
        }
        if !do_lt.boolean_value() {
            continue;
        }
        if state.get() != HsmsCommunicateState::Selected {
            break;
        }

        let req = match build_linktest_request(sys.next(is_equip, sid)) {
            Ok(m) => m,
            Err(_) => break,
        };
        let t6d = t6.get().as_duration();
        // Note: send itself touches activity (so the next wait restarts after this linktest).
        let ok = match io.send(&req, t6d) {
            Ok(Some(rsp)) if rsp.message_type() == HsmsMessageType::LinktestRsp => true,
            Ok(_) => false,
            Err(_) => false,
        };
        if ok {
            ok_cnt.fetch_add(1, Ordering::SeqCst);
        } else {
            // Parity: Linktest() false → Shutdown channel.
            io.shutdown();
            state.set(HsmsCommunicateState::NotSelected);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn hsmsss_newinstance_passive() {
        let cfg = HsmsSsCommunicatorConfig::new();
        cfg.set_session_id(10).unwrap();
        cfg.set_connection_mode(HsmsConnectionMode::Passive);
        cfg.set_socket_address(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5000));
        let comm = HsmsSsCommunicator::new_instance(cfg);
        assert!(!comm.is_open());
        assert_eq!(comm.session_id(), 10);
        assert_eq!(
            comm.hsms_communicate_state(),
            HsmsCommunicateState::NotConnected
        );
        let _ = comm.gem(); // non-null
    }

    #[test]
    fn hsmsss_newinstance_active() {
        let cfg = HsmsSsCommunicatorConfig::new();
        cfg.set_session_id(20).unwrap();
        cfg.set_connection_mode(HsmsConnectionMode::Active);
        let comm = HsmsSsCommunicator::new_instance(cfg);
        assert!(!comm.is_open());
        assert_eq!(comm.session_id(), 20);
    }

    #[test]
    fn hsmsss_sessionid_validation() {
        let cfg = HsmsSsCommunicatorConfig::new();
        assert!(cfg.set_session_id(0x10000).is_err());
        assert!(cfg.set_session_id(-1).is_err());
        cfg.set_session_id(0x7FFF).unwrap();
        assert_eq!(cfg.session_id_prop().int_value(), 0x7FFF);
    }

    #[test]
    fn hsms_connectionmode_default_passive() {
        let cfg = HsmsSsCommunicatorConfig::new();
        assert_eq!(cfg.connection_mode(), HsmsConnectionMode::Passive);
    }

    #[test]
    fn hsms_config_timeout_set() {
        let cfg = HsmsSsCommunicatorConfig::new();
        cfg.timeout().set_t3(45.0);
        assert_eq!(cfg.timeout().t3().get().milli_seconds(), 45_000);
        cfg.linktest(30.0);
        assert!(cfg.do_linktest().boolean_value());
        assert_eq!(cfg.linktest_time().get().milli_seconds(), 30_000);
    }

    /// Active↔Passive SELECT handshake smoke (real TCP, ephemeral port).
    #[test]
    fn hsmsss_active_passive_select_smoke() {
        // Probe free port, then both ends use the same address.
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = std::sync::Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(5.0);
        let active = HsmsSsCommunicator::new_instance(a_cfg);

        let p_arc = std::sync::Arc::clone(&passive);
        let p = thread::spawn(move || {
            p_arc.open_passive_select_once().unwrap();
            assert_eq!(
                p_arc.hsms_communicate_state(),
                HsmsCommunicateState::Selected
            );
        });

        // Wait until passive has bound (is_open after listen).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(
                std::time::Instant::now() < deadline,
                "passive did not bind in time"
            );
            thread::sleep(Duration::from_millis(5));
        }

        active.open_active_select_once().unwrap();
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );

        p.join().unwrap();
        assert!(passive.is_open());
        assert!(active.is_open());
        assert!(active.hsms_communicate_state().communicatable());
        assert_eq!(
            passive.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
    }

    /// Long-lived selected session: DATA + LINKTEST + SEPARATE over real TCP.
    #[test]
    fn hsmsss_live_session_data_linktest() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        p_cfg.timeout().set_t6(5.0);
        p_cfg.timeout().set_t3(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(5.0);
        a_cfg.timeout().set_t3(5.0);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));

        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || {
            p_arc.open_passive().unwrap();
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(std::time::Instant::now() < deadline, "passive bind timeout");
            thread::sleep(Duration::from_millis(5));
        }

        active.open_active().unwrap();
        p.join().unwrap();

        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        assert_eq!(
            passive.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );

        // Active → Passive DATA (no W)
        let none = active
            .send_data(1, 1, false, Secs2::ascii("PING").unwrap())
            .unwrap();
        assert!(none.is_none());

        let got = passive
            .poll_data_message(Duration::from_secs(2))
            .unwrap()
            .expect("DATA primary");
        assert_eq!(got.get_stream(), 1);
        assert_eq!(got.get_function(), 1);
        assert_eq!(got.secs2().get_ascii().unwrap(), "PING");

        // LINKTEST both ways (passive selected loop auto-replies)
        assert!(active.send_linktest().unwrap());
        assert!(passive.send_linktest().unwrap());

        // DATA with W-bit: passive replies S1F2
        let p_reply = Arc::clone(&passive);
        let reply_th = thread::spawn(move || {
            let primary = p_reply
                .poll_data_message(Duration::from_secs(2))
                .unwrap()
                .expect("W DATA");
            assert!(primary.wbit());
            p_reply
                .send_data_reply(&primary, 1, 2, false, Secs2::ascii("PONG").unwrap())
                .unwrap();
        });

        let rsp = active
            .send_data(1, 1, true, Secs2::empty())
            .unwrap()
            .expect("T3 reply");
        assert_eq!(rsp.get_function(), 2);
        assert_eq!(rsp.secs2().get_ascii().unwrap(), "PONG");
        reply_th.join().unwrap();

        active.separate().unwrap();
        // Passive selected loop sees SEPARATE → NotSelected
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while passive.hsms_communicate_state() == HsmsCommunicateState::Selected {
            assert!(
                std::time::Instant::now() < deadline,
                "passive separate timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_is_equip_default_false() {
        let cfg = HsmsSsCommunicatorConfig::new();
        assert!(!cfg.is_equip());
        cfg.set_is_equip(true);
        assert!(cfg.is_equip());
        cfg.set_is_equip(false);
        assert!(!cfg.is_equip_prop().boolean_value());
    }

    #[test]
    fn hsmsss_equip_system_bytes_on_data() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(0x0A0B).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(0x0A0B).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.set_is_equip(true);
        a_cfg.timeout().set_t6(5.0);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));
        assert!(active.is_equip());

        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || p_arc.open_passive().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        active.open_active().unwrap();
        p.join().unwrap();

        active
            .send_data(1, 1, false, Secs2::ascii("EQ").unwrap())
            .unwrap();
        let got = passive
            .poll_data_message(Duration::from_secs(2))
            .unwrap()
            .expect("data");
        // Equip system-bytes: high 2 = session 0x0A0B, low 2 = auto (first data after select uses next serial)
        let h = got.header10_bytes();
        assert_eq!([h[6], h[7]], [0x0A, 0x0B]);

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_periodic_linktest() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        p_cfg.timeout().set_t6(2.0);
        // Passive also enables linktest so both sides exercise auto-reply path.
        p_cfg.linktest(0.05); // 50ms
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(2.0);
        a_cfg.linktest(0.05);
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

        // Wait for several auto linktests.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while active.linktest_ok_count() < 2 && passive.linktest_ok_count() < 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "periodic linktest did not succeed; active={} passive={}",
                active.linktest_ok_count(),
                passive.linktest_ok_count()
            );
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        assert!(
            active.linktest_ok_count() + passive.linktest_ok_count() >= 2
        );

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_pass_through_try_sended_receive() {
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

        let try_n = Arc::new(AtomicU64::new(0));
        let sent_n = Arc::new(AtomicU64::new(0));
        let recv_n = Arc::new(AtomicU64::new(0));
        let t = Arc::clone(&try_n);
        let s = Arc::clone(&sent_n);
        let r = Arc::clone(&recv_n);
        active.add_try_send_pass_through(move |m| {
            if m.is_data_message() {
                t.fetch_add(1, Ordering::SeqCst);
            }
        });
        active.add_sended_pass_through(move |m| {
            if m.is_data_message() {
                s.fetch_add(1, Ordering::SeqCst);
            }
        });
        passive.add_receive_pass_through(move |m| {
            if m.is_data_message() {
                r.fetch_add(1, Ordering::SeqCst);
            }
        });

        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || p_arc.open_passive().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        active.open_active().unwrap();
        p.join().unwrap();

        let pass = Arc::clone(&passive);
        let drain = thread::spawn(move || {
            let _ = pass.take_data_message().unwrap();
        });
        active
            .send_data(1, 13, false, Secs2::ascii("PT").unwrap())
            .unwrap();
        drain.join().unwrap();

        assert!(
            try_n.load(Ordering::SeqCst) >= 1,
            "try-send pass-through"
        );
        assert!(
            sent_n.load(Ordering::SeqCst) >= 1,
            "sended pass-through"
        );
        // wait briefly for receive path
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while recv_n.load(Ordering::SeqCst) < 1 {
            assert!(std::time::Instant::now() < deadline, "receive pass-through");
            thread::sleep(Duration::from_millis(5));
        }

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_linktest_deferred_by_io_activity() {
        // Frequent DATA traffic must restart the linktest idle timer (LinktestReset).
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        p_cfg.timeout().set_t6(2.0);
        p_cfg.not_linktest(); // only active measures auto-linktest
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(2.0);
        a_cfg.linktest(0.15); // 150ms idle required
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

        // Busy traffic for ~400ms with period << 150ms → no auto linktest on active.
        let busy_until = std::time::Instant::now() + Duration::from_millis(400);
        while std::time::Instant::now() < busy_until {
            let pass = Arc::clone(&passive);
            let drain = thread::spawn(move || {
                let _ = pass.poll_data_message(Duration::from_millis(80));
            });
            active
                .send_data(1, 1, false, Secs2::ascii("busy").unwrap())
                .unwrap();
            drain.join().unwrap();
            thread::sleep(Duration::from_millis(40));
        }
        assert_eq!(
            active.linktest_ok_count(),
            0,
            "I/O activity must defer periodic linktest"
        );

        // After quiet period, linktest should fire.
        let quiet_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while active.linktest_ok_count() < 1 {
            assert!(
                std::time::Instant::now() < quiet_deadline,
                "linktest did not fire after idle"
            );
            thread::sleep(Duration::from_millis(20));
        }

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_data_receive_listener() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let (ltx, lrx) = std::sync::mpsc::channel::<String>();
        let id = passive.add_hsms_message_receive_listener(move |msg| {
            let s = msg.secs2().get_ascii().unwrap_or_default().to_string();
            let _ = ltx.send(s);
        });

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
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

        active
            .send_data(1, 13, false, Secs2::ascii("LISTENER").unwrap())
            .unwrap();

        let got = lrx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got, "LISTENER");
        // Still available on poll queue
        let polled = passive
            .poll_data_message(Duration::from_millis(100))
            .unwrap()
            .expect("queue");
        assert_eq!(polled.secs2().get_ascii().unwrap(), "LISTENER");

        assert!(passive.remove_hsms_message_receive_listener(id));
        assert!(!passive.remove_hsms_message_receive_listener(id));

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_active_t5_retry_then_select() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t5(0.05); // 50ms between retries
        a_cfg.timeout().set_t6(5.0);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));

        // Start Active Open with T5 retry before Passive is up.
        active.open_active_with_t5_retry().unwrap();
        thread::sleep(Duration::from_millis(120)); // a few failed connects

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));
        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || p_arc.open_passive().unwrap());

        active.wait_until_hsms_communicate_state(HsmsCommunicateState::Selected);
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        p.join().unwrap();
        assert_eq!(
            passive.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );

        active.close();
        passive.close();
    }

    #[test]
    fn hsmsss_rebind_config_defaults() {
        let cfg = HsmsSsCommunicatorConfig::new();
        assert!(cfg.do_rebind_if_passive().boolean_value());
        assert_eq!(cfg.rebind_if_passive_time().get().milli_seconds(), 10_000);
        cfg.not_rebind_if_passive();
        assert!(!cfg.do_rebind_if_passive().boolean_value());
        cfg.rebind_if_passive(5.0);
        assert!(cfg.do_rebind_if_passive().boolean_value());
        assert_eq!(cfg.rebind_if_passive_time().get().milli_seconds(), 5_000);
    }

    #[test]
    fn hsmsss_passive_rebind_second_session() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t5(0.05);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));
        passive.open_passive_with_rebind().unwrap();

        // Session 1
        let a1 = {
            let cfg = HsmsSsCommunicatorConfig::new();
            cfg.set_session_id(10).unwrap();
            cfg.set_connection_mode(HsmsConnectionMode::Active);
            cfg.set_socket_address(addr);
            cfg.timeout().set_t5(0.05);
            cfg.timeout().set_t6(5.0);
            Arc::new(HsmsSsCommunicator::new_instance(cfg))
        };
        a1.open_active_with_t5_retry().unwrap();
        a1.wait_until_hsms_communicate_state(HsmsCommunicateState::Selected);
        let sess1_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while passive.passive_session_count() < 1 {
            assert!(
                std::time::Instant::now() < sess1_deadline,
                "passive session count not updated for session1"
            );
            thread::sleep(Duration::from_millis(5));
        }
        a1.separate().unwrap();
        a1.close();

        // Wait rebind gap + second accept readiness
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while passive.hsms_communicate_state() == HsmsCommunicateState::Selected {
            assert!(std::time::Instant::now() < deadline, "session1 end timeout");
            thread::sleep(Duration::from_millis(10));
        }

        // Session 2 after rebind
        let a2 = {
            let cfg = HsmsSsCommunicatorConfig::new();
            cfg.set_session_id(10).unwrap();
            cfg.set_connection_mode(HsmsConnectionMode::Active);
            cfg.set_socket_address(addr);
            cfg.timeout().set_t5(0.05);
            cfg.timeout().set_t6(5.0);
            Arc::new(HsmsSsCommunicator::new_instance(cfg))
        };
        a2.open_active_with_t5_retry().unwrap();
        a2.wait_until_hsms_communicate_state(HsmsCommunicateState::Selected);
        assert!(
            passive.passive_session_count() >= 2,
            "expected rebind session, count={}",
            passive.passive_session_count()
        );
        assert_eq!(
            passive.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );

        a2.close();
        passive.close();
    }

    #[test]
    fn hsmsss_second_connection_already_used() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));
        passive.open_passive_listen().unwrap();

        // Wait until listening (open flag set before bind; poll connect).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if TcpStream::connect(addr).is_ok() {
                // Spurious connect — peer will T7 fail; ensure bind is up.
                break;
            }
            assert!(std::time::Instant::now() < deadline, "listen timeout");
            thread::sleep(Duration::from_millis(10));
        }
        // Brief settle after probe connect
        thread::sleep(Duration::from_millis(50));

        let mk_active = || {
            let cfg = HsmsSsCommunicatorConfig::new();
            cfg.set_session_id(10).unwrap();
            cfg.set_connection_mode(HsmsConnectionMode::Active);
            cfg.set_socket_address(addr);
            cfg.timeout().set_t6(5.0);
            HsmsSsCommunicator::new_instance(cfg)
        };

        let a1 = mk_active();
        a1.open_active().unwrap();
        assert_eq!(
            a1.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        // Wait until passive records the selected session (avoid race with listen worker).
        let sess_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while passive.passive_session_count() < 1 {
            assert!(
                std::time::Instant::now() < sess_deadline,
                "passive session count not updated"
            );
            thread::sleep(Duration::from_millis(5));
        }

        // Second Active while first still selected → ALREADY_USED → select fails.
        let a2 = mk_active();
        let err = a2.open_active().unwrap_err();
        assert!(
            matches!(err, HsmsError::Protocol(_)),
            "expected select failure, got {err:?}"
        );
        // First session remains selected.
        assert_eq!(
            a1.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        assert_eq!(passive.passive_session_count(), 1);

        a1.close();
        passive.close();
        assert!(passive.is_closed());
    }

    #[test]
    fn hsmsss_is_closed_after_close() {
        let cfg = HsmsSsCommunicatorConfig::new();
        let comm = HsmsSsCommunicator::new_instance(cfg);
        assert!(!comm.is_closed());
        comm.close();
        assert!(comm.is_closed());
        assert!(!comm.is_open());
    }
}
