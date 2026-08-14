//! HSMS-GS (SEMI-E37.2) multi-session communicator.
//!
//! Multi-session shell + GS SELECT + selected MainTask (DATA/SELECT/DESELECT/LINKTEST/SEPARATE).

use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::hsms::{
    active_select_gs, build_data_message, build_data_reply, build_deselect_request_gs,
    build_deselect_response, build_linktest_request, build_linktest_response,
    build_reject_request, build_select_request_gs, build_select_response,
    build_separate_request_gs, passive_select_gs, DeselectStatus, Error as HsmsError,
    HsmsCommunicateState, HsmsConnectionMode, HsmsMessage, HsmsMessageType, HsmsSessionIo,
    HsmsTcpChannel, RejectReason, SelectStatus, SystemBytesCounter,
};
use crate::property::{BooleanProperty, ObjectProperty, TimeoutAndUnit, TimeoutProperty};
use crate::secs2::Secs2;
use crate::timeout::SecsTimeout;

/// Session-ID out of 0..=0xFFFF (`AddSessionIdIllegalArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddSessionIdIllegalArgument;

impl std::fmt::Display for AddSessionIdIllegalArgument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal add session id")
    }
}

impl std::error::Error for AddSessionIdIllegalArgument {}

/// Unknown session (`HsmsGsUnknownSessionIdException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownSessionId;

impl std::fmt::Display for UnknownSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown session id")
    }
}

impl std::error::Error for UnknownSessionId {}

/// HSMS session handle (shell + communicate state).
pub struct HsmsSession {
    session_id: i32,
    state: ObjectProperty<HsmsCommunicateState>,
    /// Channel claimed (`SetChannel`); GS multi-session exclusive per session.
    channel_held: AtomicBool,
}

impl HsmsSession {
    pub fn new(session_id: i32) -> Self {
        Self {
            session_id,
            state: ObjectProperty::new(HsmsCommunicateState::NotConnected),
            channel_held: AtomicBool::new(false),
        }
    }

    pub fn session_id(&self) -> i32 {
        self.session_id
    }

    pub fn hsms_communicate_state(&self) -> HsmsCommunicateState {
        self.state.get()
    }

    pub fn set_hsms_communicate_state(&self, state: HsmsCommunicateState) {
        self.state.set(state);
    }

    pub fn is_selected(&self) -> bool {
        self.state.get() == HsmsCommunicateState::Selected
    }

    /// `SetChannel` — true if claimed.
    pub fn try_set_channel(&self) -> bool {
        self.channel_held
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn unset_channel(&self) {
        self.channel_held.store(false, Ordering::SeqCst);
        if self.state.get() == HsmsCommunicateState::Selected {
            self.state.set(HsmsCommunicateState::NotSelected);
        }
    }

    pub fn channel_held(&self) -> bool {
        self.channel_held.load(Ordering::SeqCst)
    }
}

/// Decide SELECT.rsp status for a GS SELECT.req (parity with MainTask SELECT branch).
///
/// - unknown session → ENTITY_UNKNOWN
/// - already selected on this connection set → ENTITY_ACTIVED
/// - session free → SUCCESS (caller SetChannel)
/// - session channel held elsewhere → ENTITY_ALREADY_USED
pub fn gs_select_status(
    session_exists: bool,
    already_selected_on_channel: bool,
    session_channel_held: bool,
) -> SelectStatus {
    if already_selected_on_channel {
        return SelectStatus::EntityActived;
    }
    if !session_exists {
        return SelectStatus::EntityUnknown;
    }
    if session_channel_held {
        return SelectStatus::EntityAlreadyUsed;
    }
    SelectStatus::Success
}

/// HSMS-GS configuration.
pub struct HsmsGsCommunicatorConfig {
    session_ids: Mutex<HashSet<i32>>,
    connection_mode: ObjectProperty<HsmsConnectionMode>,
    socket_addr: Mutex<Option<SocketAddr>>,
    timeout: SecsTimeout,
    linktest_time: TimeoutProperty,
    do_linktest: BooleanProperty,
    is_equip: BooleanProperty,
}

impl Default for HsmsGsCommunicatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl HsmsGsCommunicatorConfig {
    pub fn new() -> Self {
        Self {
            session_ids: Mutex::new(HashSet::new()),
            connection_mode: ObjectProperty::new(HsmsConnectionMode::Passive),
            socket_addr: Mutex::new(None),
            timeout: SecsTimeout::new(),
            linktest_time: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(120.0)),
            do_linktest: BooleanProperty::new(false),
            is_equip: BooleanProperty::new(false),
        }
    }

    pub fn set_is_equip(&self, equip: bool) {
        self.is_equip.set(equip);
    }

    pub fn is_equip(&self) -> bool {
        self.is_equip.boolean_value()
    }

    pub fn add_session_id(&self, session_id: i32) -> Result<bool, AddSessionIdIllegalArgument> {
        if !(0..=0xFFFF).contains(&session_id) {
            return Err(AddSessionIdIllegalArgument);
        }
        Ok(self
            .session_ids
            .lock()
            .expect("session ids")
            .insert(session_id))
    }

    pub fn remove_session_id(&self, session_id: i32) -> bool {
        self.session_ids
            .lock()
            .expect("session ids")
            .remove(&session_id)
    }

    pub fn session_ids(&self) -> HashSet<i32> {
        self.session_ids.lock().expect("session ids").clone()
    }

    pub fn set_connection_mode(&self, mode: HsmsConnectionMode) {
        self.connection_mode.set(mode);
    }

    pub fn connection_mode(&self) -> HsmsConnectionMode {
        self.connection_mode.get()
    }

    pub fn set_socket_address(&self, addr: SocketAddr) {
        *self.socket_addr.lock().expect("socket") = Some(addr);
    }

    pub fn socket_address(&self) -> Option<SocketAddr> {
        *self.socket_addr.lock().expect("socket")
    }

    pub fn timeout(&self) -> &SecsTimeout {
        &self.timeout
    }

    pub fn linktest(&self, seconds: f32) {
        self.linktest_time.set_seconds_f32(seconds);
        self.do_linktest.set_true();
    }
}

/// HSMS-GS communicator: multi-session + one-shot select + long-lived selected MainTask.
pub struct HsmsGsCommunicator {
    sessions: Arc<HashMap<i32, HsmsSession>>,
    mode: HsmsConnectionMode,
    open: AtomicBool,
    socket_addr: Option<SocketAddr>,
    t3: Duration,
    t6: Duration,
    t7: Duration,
    t8: Duration,
    is_equip: bool,
    sys_bytes: SystemBytesCounter,
    /// Shared channel after successful select.
    io: Mutex<Option<Arc<HsmsSessionIo>>>,
    /// Session-IDs selected on the current channel.
    selected_on_channel: Arc<Mutex<HashSet<i32>>>,
    /// DATA primaries delivered by GS MainTask.
    data_rx: Mutex<Option<Receiver<HsmsMessage>>>,
    loop_handle: Mutex<Option<JoinHandle<()>>>,
}

impl HsmsGsCommunicator {
    /// `HsmsGsCommunicator.NewInstance(config)`.
    pub fn new_instance(config: &HsmsGsCommunicatorConfig) -> Self {
        let mode = config.connection_mode();
        let mut sessions = HashMap::new();
        for id in config.session_ids() {
            sessions.insert(id, HsmsSession::new(id));
        }
        Self {
            sessions: Arc::new(sessions),
            mode,
            open: AtomicBool::new(false),
            socket_addr: config.socket_address(),
            t3: config.timeout().t3().get().as_duration(),
            t6: config.timeout().t6().get().as_duration(),
            t7: config.timeout().t7().get().as_duration(),
            t8: config.timeout().t8().get().as_duration(),
            is_equip: config.is_equip(),
            sys_bytes: SystemBytesCounter::new(),
            io: Mutex::new(None),
            selected_on_channel: Arc::new(Mutex::new(HashSet::new())),
            data_rx: Mutex::new(None),
            loop_handle: Mutex::new(None),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    pub fn connection_mode(&self) -> HsmsConnectionMode {
        self.mode
    }

    pub fn selected_session_ids(&self) -> HashSet<i32> {
        self.selected_on_channel
            .lock()
            .expect("selected")
            .clone()
    }

    pub fn get_hsms_sessions(&self) -> Vec<&HsmsSession> {
        self.sessions.values().collect()
    }

    pub fn exist_hsms_session(&self, session_id: i32) -> bool {
        self.sessions.contains_key(&session_id)
    }

    pub fn get_hsms_session(&self, session_id: i32) -> Result<&HsmsSession, UnknownSessionId> {
        self.sessions
            .get(&session_id)
            .ok_or(UnknownSessionId)
    }

    pub fn optional_hsms_session(&self, session_id: i32) -> Option<&HsmsSession> {
        self.sessions.get(&session_id)
    }

    /// Evaluate SELECT.rsp status for `session_id` on a channel that already has
    /// `selected_on_channel` session ids.
    pub fn select_status_for(
        &self,
        session_id: i32,
        selected_on_channel: &[i32],
    ) -> SelectStatus {
        let already = selected_on_channel.contains(&session_id);
        let exists = self.exist_hsms_session(session_id);
        let held = self
            .optional_hsms_session(session_id)
            .map(|s| s.channel_held())
            .unwrap_or(false);
        gs_select_status(exists, already, held)
    }

    /// Claim session and mark SELECTED (after SUCCESS reply path).
    pub fn select_session(&self, session_id: i32) -> Result<(), UnknownSessionId> {
        let s = self.get_hsms_session(session_id)?;
        if !s.try_set_channel() {
            return Ok(());
        }
        s.set_hsms_communicate_state(HsmsCommunicateState::Selected);
        Ok(())
    }

    /// Unset channel / leave selected (DESELECT or SEPARATE path).
    pub fn deselect_session(&self, session_id: i32) -> Result<(), UnknownSessionId> {
        let s = self.get_hsms_session(session_id)?;
        s.unset_channel();
        self.selected_on_channel
            .lock()
            .expect("selected")
            .remove(&session_id);
        Ok(())
    }

    fn select_status_callback(&self, sid: i32) -> SelectStatus {
        let selected: Vec<i32> = self
            .selected_on_channel
            .lock()
            .expect("selected")
            .iter()
            .copied()
            .collect();
        self.select_status_for(sid, &selected)
    }

    fn install_live(&self, stream: TcpStream) -> Result<(), HsmsError> {
        self.teardown_live_io_only();
        let io = Arc::new(HsmsSessionIo::with_pass_through_t8(
            stream,
            Arc::new(crate::hsms::HsmsPassThrough::new()),
            self.t8,
        )?);
        let (data_tx, data_rx) = mpsc::channel();
        let io_loop = Arc::clone(&io);
        let sessions = Arc::clone(&self.sessions);
        let selected = Arc::clone(&self.selected_on_channel);
        let handle = thread::spawn(move || {
            gs_main_loop(io_loop, sessions, selected, data_tx);
        });
        *self.io.lock().expect("io") = Some(io);
        *self.data_rx.lock().expect("data_rx") = Some(data_rx);
        *self.loop_handle.lock().expect("loop") = Some(handle);
        Ok(())
    }

    /// Passive one-shot: bind/accept → GS SELECT → SessionIo (no MainTask; prefer [`open_passive`]).
    pub fn open_passive_select_once(&self) -> Result<SelectStatus, HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let addr = self
            .socket_addr
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let listener = TcpListener::bind(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);
        let (stream, _) = listener.accept().map_err(HsmsError::from)?;

        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let (sid, status) =
            passive_select_gs(&mut ch, self.t7, |sid| self.select_status_callback(sid))?;
        drop(ch);

        if status == SelectStatus::Success {
            let _ = self.select_session(sid);
            self.selected_on_channel
                .lock()
                .expect("selected")
                .insert(sid);
            let io = HsmsSessionIo::with_pass_through_t8(
                stream,
                Arc::new(crate::hsms::HsmsPassThrough::new()),
                self.t8,
            )?;
            *self.io.lock().expect("io") = Some(Arc::new(io));
        }
        Ok(status)
    }

    /// Active one-shot: connect → GS SELECT (no MainTask; prefer [`open_active`]).
    pub fn open_active_select_once(&self, session_id: i32) -> Result<SelectStatus, HsmsError> {
        if self.mode != HsmsConnectionMode::Active {
            return Err(HsmsError::Protocol("connection mode is not ACTIVE"));
        }
        let addr = self
            .socket_addr
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let stream = TcpStream::connect(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);

        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let sys = self.sys_bytes.next(self.is_equip, session_id);
        let status = active_select_gs(&mut ch, session_id, sys, self.t6)?;
        drop(ch);

        if status == SelectStatus::Success || status == SelectStatus::EntityActived {
            if status == SelectStatus::Success {
                let _ = self.select_session(session_id);
            }
            self.selected_on_channel
                .lock()
                .expect("selected")
                .insert(session_id);
            let io = HsmsSessionIo::with_pass_through_t8(
                stream,
                Arc::new(crate::hsms::HsmsPassThrough::new()),
                self.t8,
            )?;
            *self.io.lock().expect("io") = Some(Arc::new(io));
        }
        Ok(status)
    }

    /// Passive long-lived: accept → GS SELECT → MainTask loop.
    pub fn open_passive(&self) -> Result<SelectStatus, HsmsError> {
        if self.mode != HsmsConnectionMode::Passive {
            return Err(HsmsError::Protocol("connection mode is not PASSIVE"));
        }
        let addr = self
            .socket_addr
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let listener = TcpListener::bind(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);
        let (stream, _) = listener.accept().map_err(HsmsError::from)?;

        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let (sid, status) =
            passive_select_gs(&mut ch, self.t7, |sid| self.select_status_callback(sid))?;
        drop(ch);

        if status == SelectStatus::Success {
            let _ = self.select_session(sid);
            self.selected_on_channel
                .lock()
                .expect("selected")
                .insert(sid);
            self.install_live(stream)?;
        }
        Ok(status)
    }

    /// Active long-lived: connect → GS SELECT for `session_id` → MainTask loop.
    pub fn open_active(&self, session_id: i32) -> Result<SelectStatus, HsmsError> {
        if self.mode != HsmsConnectionMode::Active {
            return Err(HsmsError::Protocol("connection mode is not ACTIVE"));
        }
        let addr = self
            .socket_addr
            .ok_or(HsmsError::Protocol("socket address unset"))?;

        let stream = TcpStream::connect(addr).map_err(HsmsError::from)?;
        self.open.store(true, Ordering::SeqCst);

        let select_stream = stream.try_clone().map_err(HsmsError::from)?;
        let mut ch = HsmsTcpChannel::new(select_stream);
        let sys = self.sys_bytes.next(self.is_equip, session_id);
        let status = active_select_gs(&mut ch, session_id, sys, self.t6)?;
        drop(ch);

        if status == SelectStatus::Success || status == SelectStatus::EntityActived {
            if status == SelectStatus::Success {
                let _ = self.select_session(session_id);
            }
            self.selected_on_channel
                .lock()
                .expect("selected")
                .insert(session_id);
            self.install_live(stream)?;
        }
        Ok(status)
    }

    /// Snapshot session Arc under lock, then run `f` **without** holding the mutex
    /// (send/reply wait may block for T3/T6 — must not stall teardown).
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

    fn next_sys_bytes(&self, session_id: i32) -> [u8; 4] {
        self.sys_bytes.next(self.is_equip, session_id)
    }

    /// Send built message (T3/T6 reply wait; WaitUntilSended always bounded).
    pub fn send(&self, msg: &HsmsMessage) -> Result<Option<HsmsMessage>, HsmsError> {
        let timeout = match HsmsSessionIo::reply_timeout_class(msg) {
            Some(crate::hsms::ReplyTimeoutClass::T3) => self.t3,
            Some(crate::hsms::ReplyTimeoutClass::T6) => self.t6,
            None if msg.is_data_message() => self.t3,
            None => self.t6,
        };
        self.with_io(|io| io.send(msg, timeout))
    }

    /// Send SELECT.req for another session on the live channel (`Session.Select`).
    ///
    /// On SUCCESS / ENTITY_ACTIVED, marks the session selected locally.
    pub fn select_req(&self, session_id: i32) -> Result<SelectStatus, HsmsError> {
        if !self.exist_hsms_session(session_id) {
            return Err(HsmsError::Protocol("unknown session id"));
        }
        let sys = self.next_sys_bytes(session_id);
        let req = build_select_request_gs(session_id, sys)?;
        let rsp = self
            .send(&req)?
            .ok_or(HsmsError::Protocol("SELECT.rsp missing"))?;
        if rsp.message_type() != HsmsMessageType::SelectRsp {
            return Err(HsmsError::Protocol("not SELECT.rsp"));
        }
        let status = SelectStatus::from_message(&rsp);
        match status {
            SelectStatus::Success => {
                let _ = self.select_session(session_id);
                self.selected_on_channel
                    .lock()
                    .expect("selected")
                    .insert(session_id);
            }
            SelectStatus::EntityActived | SelectStatus::Actived => {
                // Already selected on peer; ensure local set tracks it.
                self.selected_on_channel
                    .lock()
                    .expect("selected")
                    .insert(session_id);
                if let Some(s) = self.sessions.get(&session_id) {
                    if !s.channel_held() {
                        let _ = s.try_set_channel();
                    }
                    s.set_hsms_communicate_state(HsmsCommunicateState::Selected);
                }
            }
            _ => {}
        }
        Ok(status)
    }

    /// Send DESELECT.req for a selected session (`Session.Deselect`).
    pub fn deselect_req(&self, session_id: i32) -> Result<DeselectStatus, HsmsError> {
        if !self.exist_hsms_session(session_id) {
            return Err(HsmsError::Protocol("unknown session id"));
        }
        let sys = self.next_sys_bytes(session_id);
        let req = build_deselect_request_gs(session_id, sys)?;
        let rsp = self
            .send(&req)?
            .ok_or(HsmsError::Protocol("DESELECT.rsp missing"))?;
        if rsp.message_type() != HsmsMessageType::DeselectRsp {
            return Err(HsmsError::Protocol("not DESELECT.rsp"));
        }
        let status = DeselectStatus::from_message(&rsp);
        if status == DeselectStatus::Success || status == DeselectStatus::NoSelected {
            let _ = self.deselect_session(session_id);
        }
        Ok(status)
    }

    /// `Send(sessionId, strm, func, wbit, secs2)` — session must be selected.
    pub fn send_data(
        &self,
        session_id: i32,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<HsmsMessage>, HsmsError> {
        if !self.selected_session_ids().contains(&session_id) {
            return Err(HsmsError::Protocol("session not selected"));
        }
        let sys = self.next_sys_bytes(session_id);
        let msg = build_data_message(session_id, strm, func, wbit, body, sys)?;
        self.send(&msg)
    }

    /// Reply to primary DATA on `session_id`.
    pub fn send_data_reply(
        &self,
        session_id: i32,
        primary: &HsmsMessage,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<(), HsmsError> {
        let msg = build_data_reply(session_id, primary, strm, func, wbit, body)?;
        self.send(&msg)?;
        Ok(())
    }

    /// LINKTEST.req / wait rsp (T6).
    pub fn send_linktest(&self) -> Result<bool, HsmsError> {
        // GS linktest uses 0xFFFF device bytes on SS builder; serial only matters.
        let sys = self.next_sys_bytes(0xFFFF);
        let req = build_linktest_request(sys)?;
        match self.send(&req)? {
            Some(rsp) if rsp.message_type() == HsmsMessageType::LinktestRsp => Ok(true),
            _ => Ok(false),
        }
    }

    /// Poll next DATA primary from MainTask.
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

    /// Block for next DATA primary.
    pub fn take_data_message(&self) -> Result<HsmsMessage, HsmsError> {
        let g = self.data_rx.lock().expect("data_rx");
        let rx = g
            .as_ref()
            .ok_or(HsmsError::Protocol("not selected / no session"))?;
        rx.recv().map_err(|_| HsmsError::DetectTerminate)
    }

    /// SEPARATE.req for `session_id` and tear down channel.
    pub fn separate(&self, session_id: i32) -> Result<(), HsmsError> {
        let sys = self.next_sys_bytes(session_id);
        let msg = build_separate_request_gs(session_id, sys)?;
        let _ = self.with_io(|io| io.send_noreply(&msg));
        self.teardown_live();
        Ok(())
    }

    fn teardown_live_io_only(&self) {
        if let Some(io) = self.io.lock().expect("io").take() {
            io.shutdown();
        }
        if let Some(h) = self.loop_handle.lock().expect("loop").take() {
            let _ = h.join();
        }
        *self.data_rx.lock().expect("data_rx") = None;
    }

    fn teardown_live(&self) {
        self.teardown_live_io_only();
        let ids: Vec<i32> = self
            .selected_on_channel
            .lock()
            .expect("selected")
            .drain()
            .collect();
        for id in ids {
            if let Some(s) = self.sessions.get(&id) {
                s.unset_channel();
            }
        }
    }

    /// Shutdown channel and clear selection.
    pub fn close(&self) {
        self.teardown_live();
        self.open.store(false, Ordering::SeqCst);
    }
}

/// GS MainTask: DATA / SELECT / DESELECT / LINKTEST / SEPARATE dispatch.
fn gs_main_loop(
    io: Arc<HsmsSessionIo>,
    sessions: Arc<HashMap<i32, HsmsSession>>,
    selected: Arc<Mutex<HashSet<i32>>>,
    data_tx: Sender<HsmsMessage>,
) {
    loop {
        let msg = match io.take_primary() {
            Ok(m) => m,
            Err(_) => break,
        };

        match msg.message_type() {
            HsmsMessageType::Data => {
                let sid = msg.session_id();
                let is_sel = selected.lock().expect("selected").contains(&sid);
                if is_sel {
                    if data_tx.send(msg).is_err() {
                        break;
                    }
                } else if let Ok(r) = build_reject_request(&msg, RejectReason::NotSelected) {
                    let _ = io.send_noreply(&r);
                }
            }
            HsmsMessageType::SelectReq => {
                let sid = msg.session_id();
                let already = selected.lock().expect("selected").contains(&sid);
                let status = if already {
                    SelectStatus::EntityActived
                } else if let Some(session) = sessions.get(&sid) {
                    if session.try_set_channel() {
                        session.set_hsms_communicate_state(HsmsCommunicateState::Selected);
                        selected.lock().expect("selected").insert(sid);
                        SelectStatus::Success
                    } else {
                        SelectStatus::EntityAlreadyUsed
                    }
                } else {
                    SelectStatus::EntityUnknown
                };
                if let Ok(r) = build_select_response(&msg, status) {
                    let _ = io.send_noreply(&r);
                }
            }
            HsmsMessageType::DeselectReq => {
                let sid = msg.session_id();
                let was = selected.lock().expect("selected").remove(&sid);
                if was {
                    if let Some(s) = sessions.get(&sid) {
                        s.unset_channel();
                    }
                    if let Ok(r) = build_deselect_response(&msg, DeselectStatus::Success) {
                        let _ = io.send_noreply(&r);
                    }
                } else if let Ok(r) = build_deselect_response(&msg, DeselectStatus::NoSelected) {
                    let _ = io.send_noreply(&r);
                }
            }
            HsmsMessageType::LinktestReq => {
                if let Ok(r) = build_linktest_response(&msg) {
                    let _ = io.send_noreply(&r);
                }
            }
            HsmsMessageType::SeparateReq => {
                // SEMI E37 SEPARATE ends the HSMS connection regardless of session-id
                // (parity with SS selected dispatch).
                let sid = msg.session_id();
                let _ = selected.lock().expect("selected").remove(&sid);
                if let Some(s) = sessions.get(&sid) {
                    s.unset_channel();
                }
                // Unset all selected sessions on this channel.
                let remaining: Vec<i32> =
                    selected.lock().expect("selected").iter().copied().collect();
                for id in remaining {
                    selected.lock().expect("selected").remove(&id);
                    if let Some(s) = sessions.get(&id) {
                        s.unset_channel();
                    }
                }
                io.shutdown();
                break;
            }
            HsmsMessageType::SelectRsp
            | HsmsMessageType::DeselectRsp
            | HsmsMessageType::LinktestRsp
            | HsmsMessageType::RejectReq => {
                if let Ok(r) = build_reject_request(&msg, RejectReason::TransactionNotOpen) {
                    let _ = io.send_noreply(&r);
                }
            }
            _ => {
                let reason = if HsmsMessageType::support_s_type(msg.s_type()) {
                    if HsmsMessageType::support_p_type(msg.p_type()) {
                        RejectReason::NotSupportTypeS
                    } else {
                        RejectReason::NotSupportTypeP
                    }
                } else {
                    RejectReason::NotSupportTypeS
                };
                if let Ok(r) = build_reject_request(&msg, reason) {
                    let _ = io.send_noreply(&r);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn hsmsgs_newinstance_multisession() {
        let cfg = HsmsGsCommunicatorConfig::new();
        cfg.add_session_id(1).unwrap();
        cfg.add_session_id(2).unwrap();
        cfg.add_session_id(3).unwrap();
        cfg.set_connection_mode(HsmsConnectionMode::Passive);
        cfg.set_socket_address(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5001));
        let g = HsmsGsCommunicator::new_instance(&cfg);

        assert_eq!(g.get_hsms_sessions().len(), 3);
        assert!(g.exist_hsms_session(1));
        assert!(g.exist_hsms_session(3));
        assert!(!g.exist_hsms_session(99));

        let s2 = g.get_hsms_session(2).unwrap();
        assert_eq!(s2.session_id(), 2);

        assert!(g.optional_hsms_session(5).is_none());
        assert!(g.get_hsms_session(99).is_err());
    }

    #[test]
    fn hsmsgs_add_remove_sessionid() {
        let cfg = HsmsGsCommunicatorConfig::new();
        assert!(cfg.add_session_id(7).unwrap());
        assert!(!cfg.add_session_id(7).unwrap());
        assert!(cfg.remove_session_id(7));
        assert!(!cfg.remove_session_id(7));
        assert!(cfg.add_session_id(-1).is_err());
        assert!(cfg.add_session_id(0x10000).is_err());
        cfg.add_session_id(0xFFFF).unwrap();
    }

    #[test]
    fn hsmsgs_select_status_entity_codes() {
        assert_eq!(
            gs_select_status(false, false, false),
            SelectStatus::EntityUnknown
        );
        assert_eq!(
            gs_select_status(true, true, false),
            SelectStatus::EntityActived
        );
        assert_eq!(
            gs_select_status(true, false, true),
            SelectStatus::EntityAlreadyUsed
        );
        assert_eq!(
            gs_select_status(true, false, false),
            SelectStatus::Success
        );
        // ENTITY_* status codes (wire byte[3])
        assert_eq!(SelectStatus::EntityUnknown.status_code(), 4);
        assert_eq!(SelectStatus::EntityAlreadyUsed.status_code(), 5);
        assert_eq!(SelectStatus::EntityActived.status_code(), 6);
    }

    #[test]
    fn hsmsgs_session_select_deselect() {
        let cfg = HsmsGsCommunicatorConfig::new();
        cfg.add_session_id(10).unwrap();
        cfg.add_session_id(20).unwrap();
        let g = HsmsGsCommunicator::new_instance(&cfg);

        assert_eq!(
            g.select_status_for(99, &[]),
            SelectStatus::EntityUnknown
        );
        assert_eq!(g.select_status_for(10, &[]), SelectStatus::Success);

        g.select_session(10).unwrap();
        assert!(g.get_hsms_session(10).unwrap().is_selected());
        assert_eq!(
            g.select_status_for(10, &[10]),
            SelectStatus::EntityActived
        );
        // Held by session even if not listed on "this" channel set
        assert_eq!(
            g.select_status_for(10, &[]),
            SelectStatus::EntityAlreadyUsed
        );

        g.deselect_session(10).unwrap();
        assert!(!g.get_hsms_session(10).unwrap().channel_held());
        assert_eq!(g.select_status_for(10, &[]), SelectStatus::Success);
    }

    #[test]
    fn hsmsgs_select_req_gs_header_device() {
        use crate::hsms::{build_select_request_gs, HsmsMessageType};
        let m = build_select_request_gs(10, [0, 0, 0, 1]).unwrap();
        assert_eq!(m.message_type(), HsmsMessageType::SelectReq);
        assert_eq!(&m.header10_bytes()[0..2], &[0x00, 0x0A]);
        assert_eq!(m.session_id(), 10);
    }

    #[test]
    fn hsmsgs_tcp_select_success_and_entity_unknown() {
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        // --- SUCCESS path ---
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.add_session_id(20).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.add_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(5.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            let st = passive.open_passive_select_once().unwrap();
            assert_eq!(st, SelectStatus::Success);
            assert!(passive.get_hsms_session(10).unwrap().is_selected());
            assert!(passive.selected_session_ids().contains(&10));
            passive.close();
        });

        thread::sleep(Duration::from_millis(30));
        let st = active.open_active_select_once(10).unwrap();
        assert_eq!(st, SelectStatus::Success);
        assert!(active.selected_session_ids().contains(&10));
        active.close();
        p.join().unwrap();

        // --- ENTITY_UNKNOWN path ---
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(5.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            let st = passive.open_passive_select_once().unwrap();
            assert_eq!(st, SelectStatus::EntityUnknown);
            assert!(passive.selected_session_ids().is_empty());
            passive.close();
        });

        thread::sleep(Duration::from_millis(30));
        let st = active.open_active_select_once(99).unwrap();
        assert_eq!(st, SelectStatus::EntityUnknown);
        assert!(active.selected_session_ids().is_empty());
        active.close();
        p.join().unwrap();
    }

    #[test]
    fn hsmsgs_selected_data_linktest_separate() {
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.add_session_id(20).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t3(5.0);
        p_cfg.timeout().set_t6(5.0);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.add_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(5.0);
        a_cfg.timeout().set_t6(5.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            let st = passive.open_passive().unwrap();
            assert_eq!(st, SelectStatus::Success);
            // Receive DATA primary
            let primary = passive
                .poll_data_message(Duration::from_secs(2))
                .unwrap()
                .expect("DATA");
            assert_eq!(primary.session_id(), 10);
            assert_eq!(primary.secs2().get_ascii().unwrap(), "GS-PING");
            passive
                .send_data_reply(
                    10,
                    &primary,
                    1,
                    2,
                    false,
                    Secs2::ascii("GS-PONG").unwrap(),
                )
                .unwrap();
            // LINKTEST auto-answered by MainTask
            thread::sleep(Duration::from_millis(100));
            // Wait SEPARATE tear-down
            thread::sleep(Duration::from_millis(200));
            passive.close();
        });

        thread::sleep(Duration::from_millis(30));
        let st = active.open_active(10).unwrap();
        assert_eq!(st, SelectStatus::Success);
        assert!(active.get_hsms_session(10).unwrap().is_selected());

        let reply = active
            .send_data(10, 1, 1, true, Secs2::ascii("GS-PING").unwrap())
            .unwrap()
            .expect("W-bit reply");
        assert_eq!(reply.secs2().get_ascii().unwrap(), "GS-PONG");
        assert_eq!(reply.get_function(), 2);

        assert!(active.send_linktest().unwrap());

        active.separate(10).unwrap();
        active.close();
        p.join().unwrap();
    }

    #[test]
    fn hsmsgs_data_not_selected_rejected() {
        // After select session 10, DATA for session 20 → REJECT NOT_SELECTED (MainTask).
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.add_session_id(20).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t6(2.0);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.add_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(2.0);
        a_cfg.timeout().set_t6(2.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            assert_eq!(passive.open_passive().unwrap(), SelectStatus::Success);
            // No DATA should be delivered for session 20 (REJECT NOT_SELECTED on wire).
            let none = passive
                .poll_data_message(Duration::from_millis(400))
                .expect("channel still up while active open");
            assert!(none.is_none());
            passive
        });

        thread::sleep(Duration::from_millis(30));
        active.open_active(10).unwrap();
        // Craft DATA with session 20 (not selected) — fire-and-forget.
        let sys = [0, 0, 0, 9];
        let bad = build_data_message(20, 1, 1, false, Secs2::ascii("X").unwrap(), sys).unwrap();
        active.send(&bad).unwrap();
        let passive = p.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn hsmsgs_multi_session_select_deselect_on_channel() {
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.add_session_id(20).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t3(5.0);
        p_cfg.timeout().set_t6(5.0);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.add_session_id(10).unwrap();
        a_cfg.add_session_id(20).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(5.0);
        a_cfg.timeout().set_t6(5.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            assert_eq!(passive.open_passive().unwrap(), SelectStatus::Success);
            // Wait for second SELECT (session 20) via MainTask, then DATA on 20.
            let primary = passive
                .poll_data_message(Duration::from_secs(3))
                .unwrap()
                .expect("DATA on session 20");
            assert_eq!(primary.session_id(), 20);
            assert_eq!(primary.secs2().get_ascii().unwrap(), "S20");
            passive
                .send_data_reply(20, &primary, 1, 2, false, Secs2::ascii("OK20").unwrap())
                .unwrap();
            // Allow DESELECT.req to be processed by MainTask.
            thread::sleep(Duration::from_millis(250));
            assert!(
                !passive.selected_session_ids().contains(&20),
                "session 20 should be deselected on passive"
            );
            assert!(passive.selected_session_ids().contains(&10));
            passive
        });

        thread::sleep(Duration::from_millis(40));
        assert_eq!(active.open_active(10).unwrap(), SelectStatus::Success);
        assert!(active.selected_session_ids().contains(&10));

        // Second session on same channel.
        let st = active.select_req(20).unwrap();
        assert_eq!(st, SelectStatus::Success);
        assert!(active.selected_session_ids().contains(&20));
        assert!(active.get_hsms_session(20).unwrap().is_selected());

        // Re-select → ENTITY_ACTIVED
        let st2 = active.select_req(20).unwrap();
        assert_eq!(st2, SelectStatus::EntityActived);

        // DATA on session 20.
        let reply = active
            .send_data(20, 1, 1, true, Secs2::ascii("S20").unwrap())
            .unwrap()
            .expect("reply");
        assert_eq!(reply.secs2().get_ascii().unwrap(), "OK20");

        // Deselect session 20; session 10 remains.
        let ds = active.deselect_req(20).unwrap();
        assert_eq!(ds, DeselectStatus::Success);
        assert!(!active.selected_session_ids().contains(&20));
        assert!(active.selected_session_ids().contains(&10));
        assert!(active
            .send_data(20, 1, 1, false, Secs2::empty())
            .unwrap_err()
            .to_string()
            .contains("not selected"));

        // Unknown local session
        assert!(active.select_req(99).is_err());

        let passive = p.join().unwrap();
        active.close();
        passive.close();
    }

    #[test]
    fn hsmsgs_select_req_entity_unknown() {
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        // Passive only knows session 10.
        let p_cfg = HsmsGsCommunicatorConfig::new();
        p_cfg.add_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t6(3.0);
        p_cfg.timeout().set_t7(5.0);
        let passive = HsmsGsCommunicator::new_instance(&p_cfg);

        // Active has 10 and 20, but peer has no 20.
        let a_cfg = HsmsGsCommunicatorConfig::new();
        a_cfg.add_session_id(10).unwrap();
        a_cfg.add_session_id(20).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t6(3.0);
        let active = HsmsGsCommunicator::new_instance(&a_cfg);

        let p = thread::spawn(move || {
            assert_eq!(passive.open_passive().unwrap(), SelectStatus::Success);
            thread::sleep(Duration::from_millis(400));
            passive
        });

        thread::sleep(Duration::from_millis(40));
        active.open_active(10).unwrap();
        let st = active.select_req(20).unwrap();
        assert_eq!(st, SelectStatus::EntityUnknown);
        assert!(!active.selected_session_ids().contains(&20));

        let passive = p.join().unwrap();
        active.close();
        passive.close();
    }
}
