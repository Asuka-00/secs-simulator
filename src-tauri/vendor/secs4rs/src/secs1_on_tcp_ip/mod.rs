//! SECS-I over TCP/IP communicator + receiver (client/server) + builder send APIs.
//!
//! Source: `Secs1OnTcpIpCommunicator` / `Secs1OnTcpIpReceiverCommunicator` (subset).

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::hsms::SystemBytesCounter;
use crate::open_close::{OpenAndCloseable, OpenCloseError, OpenCloseState};
use crate::property::{
    BooleanProperty, IntegerProperty, ObjectProperty, TimeoutAndUnit, TimeoutProperty,
};
use crate::secs1::{
    build_primary, build_reply, check_device_id, recv_block_handshake, recv_message,
    send_block_handshake_role, send_message_role, send_message_with_slave_yield,
    DeviceIdIllegalArgument, Error as Secs1Error, Secs1Circuit, Secs1CircuitConfig, Secs1Message,
    Secs1MessageBlock,
};
use crate::secs2::Secs2;
use crate::timeout::SecsTimeout;

/// SECS-I OnTcpIp configuration (equipment/host + timeouts).
pub struct Secs1OnTcpIpCommunicatorConfig {
    socket_addr: Mutex<Option<SocketAddr>>,
    timeout: SecsTimeout,
    /// Master side of ENQ contention (`IsMaster`).
    is_master: BooleanProperty,
    is_equip: BooleanProperty,
    device_id: ObjectProperty<i32>,
    /// Retry count for ENQ/EOT/ACK (default 3).
    retry: IntegerProperty,
    reconnect: BooleanProperty,
    /// Sleep between connect attempts (`ReconnectSeconds`, default 5.0s).
    reconnect_seconds: TimeoutProperty,
    /// Drop blocks not matching Device-ID after ACK (default true).
    check_block_device_id: BooleanProperty,
}

impl Default for Secs1OnTcpIpCommunicatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Secs1OnTcpIpCommunicatorConfig {
    pub fn new() -> Self {
        Self {
            socket_addr: Mutex::new(None),
            timeout: SecsTimeout::new(),
            is_master: BooleanProperty::new(true),
            is_equip: BooleanProperty::new(false),
            device_id: ObjectProperty::new(10),
            retry: IntegerProperty::new(3),
            reconnect: BooleanProperty::new(true),
            reconnect_seconds: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(5.0)),
            check_block_device_id: BooleanProperty::new(true),
        }
    }

    pub fn set_socket_address(&self, addr: SocketAddr) {
        *self.socket_addr.lock().expect("addr") = Some(addr);
    }

    pub fn socket_address(&self) -> Option<SocketAddr> {
        *self.socket_addr.lock().expect("addr")
    }

    pub fn timeout(&self) -> &SecsTimeout {
        &self.timeout
    }

    pub fn set_is_master(&self, master: bool) {
        self.is_master.set(master);
    }

    pub fn is_master(&self) -> bool {
        self.is_master.boolean_value()
    }

    pub fn set_is_equip(&self, equip: bool) {
        self.is_equip.set(equip);
    }

    pub fn is_equip(&self) -> bool {
        self.is_equip.boolean_value()
    }

    pub fn set_device_id(&self, id: i32) -> Result<(), DeviceIdIllegalArgument> {
        check_device_id(id)?;
        self.device_id.set(id);
        Ok(())
    }

    pub fn device_id(&self) -> i32 {
        self.device_id.get()
    }

    pub fn set_retry(&self, retry: i32) {
        self.retry.set(retry.max(0));
    }

    pub fn retry(&self) -> i32 {
        self.retry.int_value()
    }

    pub fn set_reconnect_enabled(&self, on: bool) {
        self.reconnect.set(on);
    }

    pub fn reconnect_enabled(&self) -> bool {
        self.reconnect.boolean_value()
    }

    pub fn set_reconnect_seconds(&self, seconds: f32) {
        self.reconnect_seconds.set_seconds_f32(seconds);
    }

    pub fn reconnect_seconds(&self) -> &TimeoutProperty {
        &self.reconnect_seconds
    }

    pub fn set_check_block_device_id(&self, check: bool) {
        self.check_block_device_id.set(check);
    }

    pub fn is_check_block_device_id(&self) -> bool {
        self.check_block_device_id.boolean_value()
    }
}

/// Thin TCP channel for SECS-I block/message handshake I/O.
pub struct Secs1TcpChannel {
    stream: TcpStream,
    t2: Duration,
    t4: Duration,
    is_master: bool,
    retry: u32,
}

impl Secs1TcpChannel {
    pub fn new(stream: TcpStream, t2: Duration) -> Self {
        Self {
            stream,
            t2,
            t4: Duration::from_secs(45),
            is_master: true,
            retry: 3,
        }
    }

    pub fn with_timeouts(stream: TcpStream, t2: Duration, t4: Duration) -> Self {
        Self {
            stream,
            t2,
            t4,
            is_master: true,
            retry: 3,
        }
    }

    pub fn set_is_master(&mut self, master: bool) {
        self.is_master = master;
    }

    pub fn set_retry(&mut self, retry: u32) {
        self.retry = retry;
    }

    pub fn connect(addr: SocketAddr, t2: Duration) -> Result<Self, Secs1Error> {
        let stream = TcpStream::connect(addr).map_err(Secs1Error::from)?;
        Ok(Self::new(stream, t2))
    }

    pub fn send_block(&mut self, block: &Secs1MessageBlock) -> Result<(), Secs1Error> {
        send_block_handshake_role(&mut self.stream, block, self.t2, self.is_master)
    }

    pub fn recv_block(&mut self) -> Result<Secs1MessageBlock, Secs1Error> {
        recv_block_handshake(&mut self.stream, self.t2)
    }

    /// Send complete multi-block message (respects `is_master` ENQ role).
    pub fn send_message(&mut self, msg: &Secs1Message) -> Result<(), Secs1Error> {
        send_message_role(&mut self.stream, msg, self.t2, self.is_master)
    }

    /// Receive complete multi-block message (T4 between blocks).
    pub fn recv_message(&mut self) -> Result<Secs1Message, Secs1Error> {
        recv_message(&mut self.stream, self.t2, self.t4)
    }

    /// Send with slave yield on ENQ contention (`send_message_with_slave_yield`).
    pub fn send_message_yield(
        &mut self,
        msg: &Secs1Message,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        send_message_with_slave_yield(
            &mut self.stream,
            msg,
            self.t2,
            self.t4,
            self.is_master,
            self.retry,
        )
    }

    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

/// Communicator with Open → circuit session; optional reconnect loop.
pub struct Secs1OnTcpIpCommunicator {
    config: Secs1OnTcpIpCommunicatorConfig,
    state: OpenCloseState,
    /// Legacy flag used by `open_channel` smoke path.
    open: AtomicBool,
    circuit: Mutex<Option<Arc<Secs1Circuit>>>,
    sys: SystemBytesCounter,
    /// TCP circuit currently up (C# channel non-empty ≈ communicate true).
    connected: BooleanProperty,
    open_stop: AtomicBool,
    open_handle: Mutex<Option<JoinHandle<()>>>,
    /// How many successful connects (including reconnects).
    connect_count: std::sync::atomic::AtomicU32,
}

impl Secs1OnTcpIpCommunicator {
    pub fn new_instance(config: Secs1OnTcpIpCommunicatorConfig) -> Self {
        Self {
            config,
            state: OpenCloseState::new(),
            open: AtomicBool::new(false),
            circuit: Mutex::new(None),
            sys: SystemBytesCounter::new(),
            connected: BooleanProperty::new(false),
            open_stop: AtomicBool::new(false),
            open_handle: Mutex::new(None),
            connect_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn is_open(&self) -> bool {
        self.state.is_open() || self.open.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.boolean_value()
    }

    pub fn connect_count(&self) -> u32 {
        self.connect_count.load(Ordering::SeqCst)
    }

    pub fn wait_until_connected(&self, timeout: Duration) -> bool {
        self.connected.wait_until_true_duration(timeout).is_ok()
    }

    pub fn config(&self) -> &Secs1OnTcpIpCommunicatorConfig {
        &self.config
    }

    fn circuit_config(&self) -> Secs1CircuitConfig {
        Secs1CircuitConfig {
            t1: self.config.timeout().t1().get().as_duration(),
            t2: self.config.timeout().t2().get().as_duration(),
            t3: self.config.timeout().t3().get().as_duration(),
            t4: self.config.timeout().t4().get().as_duration(),
            is_master: self.config.is_master(),
            retry: self.config.retry().max(0) as u32,
            device_id: self.config.device_id(),
            check_block_device_id: self.config.is_check_block_device_id(),
        }
    }

    /// Connect and return a block/message channel (Active-style open once).
    pub fn open_channel(&self) -> Result<Secs1TcpChannel, Secs1Error> {
        let addr = self
            .config
            .socket_address()
            .ok_or(Secs1Error::Protocol("socket address unset"))?;
        let t2 = self.config.timeout().t2().get().as_duration();
        let t4 = self.config.timeout().t4().get().as_duration();
        let stream = TcpStream::connect(addr).map_err(Secs1Error::from)?;
        let mut ch = Secs1TcpChannel::with_timeouts(stream, t2, t4);
        ch.set_is_master(self.config.is_master());
        ch.set_retry(self.config.retry().max(0) as u32);
        self.open.store(true, Ordering::SeqCst);
        Ok(ch)
    }

    fn install_circuit(&self, stream: TcpStream) -> Result<(), OpenCloseError> {
        self.teardown_circuit_only();
        let circuit =
            Secs1Circuit::start(stream, self.circuit_config()).map_err(|_| OpenCloseError::Failed)?;
        *self.circuit.lock().expect("circuit") = Some(circuit);
        self.connected.set(true);
        self.connect_count.fetch_add(1, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn teardown_circuit_only(&self) {
        if let Some(c) = self.circuit.lock().expect("circuit").take() {
            c.close();
        }
        self.connected.set(false);
    }

    fn circuit_alive(&self) -> bool {
        self.circuit
            .lock()
            .expect("circuit")
            .as_ref()
            .map(|c| c.is_alive())
            .unwrap_or(false)
    }

    /// Connect once and install circuit (no mark_open; used by reconnect worker).
    fn connect_once(&self) -> Result<(), OpenCloseError> {
        let addr = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        let stream = TcpStream::connect(addr).map_err(|_| OpenCloseError::Failed)?;
        self.install_circuit(stream)
    }

    /// Open: connect once and start circuit (reader + protocol loop).
    pub fn open(&self) -> Result<(), OpenCloseError> {
        self.state.mark_open()?;
        if let Err(e) = self.connect_once() {
            self.state.mark_closed();
            return Err(e);
        }
        Ok(())
    }

    /// Open from an already-accepted stream (Receiver / test harness).
    pub fn open_with_stream(&self, stream: TcpStream) -> Result<(), OpenCloseError> {
        self.state.mark_open()?;
        if let Err(e) = self.install_circuit(stream) {
            self.state.mark_closed();
            return Err(e);
        }
        Ok(())
    }

    /// Full Open with reconnect loop (`ReconnectSeconds` between attempts).
    ///
    /// Call on `Arc<Self>`. After peer drop, sleeps and reconnects until [`close`].
    pub fn open_with_reconnect(self: &Arc<Self>) -> Result<(), OpenCloseError> {
        let _ = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        self.state.mark_open()?;
        self.open_stop.store(false, Ordering::SeqCst);
        self.open.store(true, Ordering::SeqCst);

        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            while !this.open_stop.load(Ordering::SeqCst) && !this.state.is_closed() {
                match this.connect_once() {
                    Ok(()) => {
                        while !this.open_stop.load(Ordering::SeqCst)
                            && this.circuit_alive()
                        {
                            thread::sleep(Duration::from_millis(20));
                        }
                        this.teardown_circuit_only();
                    }
                    Err(_) => {
                        // connect failed — sleep then retry
                    }
                }
                if this.open_stop.load(Ordering::SeqCst) || this.state.is_closed() {
                    break;
                }
                if !this.config.reconnect_enabled() {
                    break;
                }
                let wait = this.config.reconnect_seconds().get().as_duration();
                sleep_chunked(wait, &this.open_stop);
            }
        });
        *self.open_handle.lock().expect("open handle") = Some(handle);
        Ok(())
    }

    pub fn close(&self) {
        self.open_stop.store(true, Ordering::SeqCst);
        self.teardown_circuit_only();
        self.state.mark_closed();
        self.open.store(false, Ordering::SeqCst);
        if let Some(h) = self.open_handle.lock().expect("open handle").take() {
            let _ = h.join();
        }
    }

    /// Snapshot circuit Arc under lock, then run `f` **without** holding the mutex
    /// (send/recv may block for T3 / primary — must not stall reconnect/`close`).
    fn with_circuit<R>(&self, f: impl FnOnce(&Secs1Circuit) -> R) -> Result<R, Secs1Error> {
        let c = {
            let g = self.circuit.lock().expect("circuit");
            g.as_ref()
                .cloned()
                .ok_or(Secs1Error::ChannelShutdown)?
        };
        Ok(f(&c))
    }

    /// Send SECS-I message through the circuit; W-bit waits for T3 reply.
    pub fn send(&self, msg: Secs1Message) -> Result<Option<Secs1Message>, Secs1Error> {
        self.with_circuit(|c| c.send(msg))?
    }

    /// Build primary from device/sys and send (`SecsCommunicator.Send`).
    pub fn send_data(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        let msg = build_primary(
            self.config.device_id(),
            self.config.is_equip(),
            &self.sys,
            strm,
            func,
            wbit,
            body,
        )?;
        self.send(msg)
    }

    /// Build reply reusing primary system-bytes and send.
    pub fn send_data_reply(
        &self,
        primary: &Secs1Message,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        let msg = build_reply(
            self.config.device_id(),
            self.config.is_equip(),
            primary,
            strm,
            func,
            wbit,
            body,
        )?;
        self.send(msg)
    }

    /// Receive next primary message (blocking).
    pub fn recv_primary(&self) -> Result<Secs1Message, Secs1Error> {
        self.with_circuit(|c| c.recv_primary())?
    }

    /// Poll primary with timeout.
    pub fn poll_primary(
        &self,
        timeout: Duration,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        self.with_circuit(|c| c.poll_primary(timeout))?
    }
}

impl OpenAndCloseable for Secs1OnTcpIpCommunicator {
    fn open(&self) -> Result<(), OpenCloseError> {
        Secs1OnTcpIpCommunicator::open(self)
    }

    fn is_open(&self) -> bool {
        Secs1OnTcpIpCommunicator::is_open(self)
    }

    fn is_closed(&self) -> bool {
        Secs1OnTcpIpCommunicator::is_closed(self)
    }

    fn close(&self) {
        Secs1OnTcpIpCommunicator::close(self)
    }
}

/// Receiver (server) communicator config (`Secs1OnTcpIpReceiverCommunicatorConfig`).
pub struct Secs1OnTcpIpReceiverCommunicatorConfig {
    socket_addr: Mutex<Option<SocketAddr>>,
    timeout: SecsTimeout,
    is_master: BooleanProperty,
    is_equip: BooleanProperty,
    device_id: ObjectProperty<i32>,
    retry: IntegerProperty,
    rebind_seconds: TimeoutProperty,
    check_block_device_id: BooleanProperty,
}

impl Default for Secs1OnTcpIpReceiverCommunicatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Secs1OnTcpIpReceiverCommunicatorConfig {
    pub fn new() -> Self {
        Self {
            socket_addr: Mutex::new(None),
            timeout: SecsTimeout::new(),
            is_master: BooleanProperty::new(false),
            is_equip: BooleanProperty::new(true),
            device_id: ObjectProperty::new(10),
            retry: IntegerProperty::new(3),
            rebind_seconds: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(5.0)),
            check_block_device_id: BooleanProperty::new(true),
        }
    }

    pub fn set_socket_address(&self, addr: SocketAddr) {
        *self.socket_addr.lock().expect("addr") = Some(addr);
    }

    pub fn socket_address(&self) -> Option<SocketAddr> {
        *self.socket_addr.lock().expect("addr")
    }

    pub fn timeout(&self) -> &SecsTimeout {
        &self.timeout
    }

    pub fn set_is_master(&self, master: bool) {
        self.is_master.set(master);
    }

    pub fn is_master(&self) -> bool {
        self.is_master.boolean_value()
    }

    pub fn set_is_equip(&self, equip: bool) {
        self.is_equip.set(equip);
    }

    pub fn is_equip(&self) -> bool {
        self.is_equip.boolean_value()
    }

    pub fn set_device_id(&self, id: i32) -> Result<(), DeviceIdIllegalArgument> {
        check_device_id(id)?;
        self.device_id.set(id);
        Ok(())
    }

    pub fn device_id(&self) -> i32 {
        self.device_id.get()
    }

    pub fn set_retry(&self, retry: i32) {
        self.retry.set(retry.max(0));
    }

    pub fn retry(&self) -> i32 {
        self.retry.int_value()
    }

    pub fn set_rebind_seconds(&self, seconds: f32) {
        self.rebind_seconds.set_seconds_f32(seconds);
    }

    pub fn rebind_seconds(&self) -> &TimeoutProperty {
        &self.rebind_seconds
    }

    pub fn set_check_block_device_id(&self, check: bool) {
        self.check_block_device_id.set(check);
    }

    pub fn is_check_block_device_id(&self) -> bool {
        self.check_block_device_id.boolean_value()
    }
}

/// SECS-I OnTcpIp receiver (bind/accept; rebind sequential or concurrent multi-accept).
pub struct Secs1OnTcpIpReceiverCommunicator {
    config: Secs1OnTcpIpReceiverCommunicatorConfig,
    state: OpenCloseState,
    circuit: Mutex<Option<Arc<Secs1Circuit>>>,
    listener: Mutex<Option<TcpListener>>,
    sys: SystemBytesCounter,
    connected: BooleanProperty,
    open_stop: AtomicBool,
    open_handle: Mutex<Option<JoinHandle<()>>>,
    /// Background workers for concurrent accept handlers (multi-accept path).
    accept_workers: Mutex<Vec<JoinHandle<()>>>,
    /// Successful accepts (including post-drop sequential accepts).
    accept_count: std::sync::atomic::AtomicU32,
}

impl Secs1OnTcpIpReceiverCommunicator {
    pub fn new_instance(config: Secs1OnTcpIpReceiverCommunicatorConfig) -> Self {
        Self {
            config,
            state: OpenCloseState::new(),
            circuit: Mutex::new(None),
            listener: Mutex::new(None),
            sys: SystemBytesCounter::new(),
            connected: BooleanProperty::new(false),
            open_stop: AtomicBool::new(false),
            open_handle: Mutex::new(None),
            accept_workers: Mutex::new(Vec::new()),
            accept_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn config(&self) -> &Secs1OnTcpIpReceiverCommunicatorConfig {
        &self.config
    }

    pub fn is_open(&self) -> bool {
        self.state.is_open()
    }

    pub fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.boolean_value()
    }

    pub fn accept_count(&self) -> u32 {
        self.accept_count.load(Ordering::SeqCst)
    }

    pub fn wait_until_connected(&self, timeout: Duration) -> bool {
        self.connected.wait_until_true_duration(timeout).is_ok()
    }

    fn circuit_config(&self) -> Secs1CircuitConfig {
        Secs1CircuitConfig {
            t1: self.config.timeout().t1().get().as_duration(),
            t2: self.config.timeout().t2().get().as_duration(),
            t3: self.config.timeout().t3().get().as_duration(),
            t4: self.config.timeout().t4().get().as_duration(),
            is_master: self.config.is_master(),
            retry: self.config.retry().max(0) as u32,
            device_id: self.config.device_id(),
            check_block_device_id: self.config.is_check_block_device_id(),
        }
    }

    fn install_circuit(&self, stream: TcpStream) -> Result<(), OpenCloseError> {
        self.teardown_circuit_only();
        let circuit =
            Secs1Circuit::start(stream, self.circuit_config()).map_err(|_| OpenCloseError::Failed)?;
        *self.circuit.lock().expect("circuit") = Some(circuit);
        self.connected.set(true);
        self.accept_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn teardown_circuit_only(&self) {
        if let Some(c) = self.circuit.lock().expect("circuit").take() {
            c.close();
        }
        self.connected.set(false);
    }

    fn circuit_alive(&self) -> bool {
        self.circuit
            .lock()
            .expect("circuit")
            .as_ref()
            .map(|c| c.is_alive())
            .unwrap_or(false)
    }

    /// Bind, accept one connection, start circuit (one-shot server open).
    pub fn open(&self) -> Result<(), OpenCloseError> {
        self.state.mark_open()?;
        let addr = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        let listener = TcpListener::bind(addr).map_err(|_| {
            self.state.mark_closed();
            OpenCloseError::Failed
        })?;
        // Publish bound address (supports port 0).
        if let Ok(local) = listener.local_addr() {
            self.config.set_socket_address(local);
        }
        let (stream, _) = listener.accept().map_err(|_| {
            self.state.mark_closed();
            OpenCloseError::Failed
        })?;
        self.install_circuit(stream).map_err(|e| {
            self.state.mark_closed();
            e
        })?;
        *self.listener.lock().expect("listener") = Some(listener);
        Ok(())
    }

    /// Bind only; return bound address (for client connect before accept).
    pub fn bind(&self) -> Result<SocketAddr, OpenCloseError> {
        self.state.mark_open()?;
        self.bind_listener()
    }

    fn bind_listener(&self) -> Result<SocketAddr, OpenCloseError> {
        let addr = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        // Drop previous listener if any.
        *self.listener.lock().expect("listener") = None;
        let listener = TcpListener::bind(addr).map_err(|_| OpenCloseError::Failed)?;
        let local = listener.local_addr().map_err(|_| OpenCloseError::Failed)?;
        self.config.set_socket_address(local);
        // Non-blocking accept poll for interruptible rebind path.
        let _ = listener.set_nonblocking(false);
        *self.listener.lock().expect("listener") = Some(listener);
        Ok(local)
    }

    /// Accept one connection after `bind` and start circuit.
    pub fn accept_one(&self) -> Result<(), OpenCloseError> {
        let listener = {
            let g = self.listener.lock().expect("listener");
            g.as_ref()
                .ok_or(OpenCloseError::Failed)?
                .try_clone()
                .map_err(|_| OpenCloseError::Failed)?
        };
        let (stream, _) = listener.accept().map_err(|_| OpenCloseError::Failed)?;
        self.install_circuit(stream)
    }

    /// Accept with short timeout so `open_stop` can interrupt (rebind worker).
    fn accept_one_interruptible(&self) -> Result<(), OpenCloseError> {
        let listener = {
            let g = self.listener.lock().expect("listener");
            g.as_ref()
                .ok_or(OpenCloseError::Failed)?
                .try_clone()
                .map_err(|_| OpenCloseError::Failed)?
        };
        let _ = listener.set_nonblocking(true);
        loop {
            if self.open_stop.load(Ordering::SeqCst) || self.state.is_closed() {
                let _ = listener.set_nonblocking(false);
                return Err(OpenCloseError::Failed);
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = listener.set_nonblocking(false);
                    return self.install_circuit(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => {
                    let _ = listener.set_nonblocking(false);
                    return Err(OpenCloseError::Failed);
                }
            }
        }
    }

    /// Full Open: bind (if needed), sequential accept loop; on failure sleep `RebindSeconds` and rebind.
    ///
    /// After peer drop, accepts the next client on the same listener without sleeping
    /// (parity with C# accept loop). Safe after [`bind`] (already open). Call on `Arc<Self>`.
    pub fn open_with_rebind(self: &Arc<Self>) -> Result<(), OpenCloseError> {
        let _ = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        if !self.state.is_open() {
            self.state.mark_open()?;
        }
        self.open_stop.store(false, Ordering::SeqCst);

        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            while !this.open_stop.load(Ordering::SeqCst) && !this.state.is_closed() {
                // Ensure listener is bound.
                if this.listener.lock().expect("listener").is_none() {
                    if this.bind_listener().is_err() {
                        if this.open_stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let wait = this.config.rebind_seconds().get().as_duration();
                        sleep_chunked(wait, &this.open_stop);
                        continue;
                    }
                }

                match this.accept_one_interruptible() {
                    Ok(()) => {
                        while !this.open_stop.load(Ordering::SeqCst) && this.circuit_alive() {
                            thread::sleep(Duration::from_millis(20));
                        }
                        this.teardown_circuit_only();
                        // Sequential accept on same bind (no rebind sleep).
                    }
                    Err(_) => {
                        // Accept failed / stopped: drop listener and rebind after interval.
                        *this.listener.lock().expect("listener") = None;
                        if this.open_stop.load(Ordering::SeqCst) || this.state.is_closed() {
                            break;
                        }
                        let wait = this.config.rebind_seconds().get().as_duration();
                        sleep_chunked(wait, &this.open_stop);
                    }
                }
            }
        });
        *self.open_handle.lock().expect("open handle") = Some(handle);
        Ok(())
    }

    /// Concurrent multi-accept Open (parity: C# `Bind` accept loop + `HandleAccepted` workers).
    ///
    /// One shared multi-circuit; each accept attaches a channel (shared PutBytes).
    /// Does **not** wait for a channel to drop before accepting the next.
    /// Connected = `channel_count > 0`. Call on `Arc<Self>` after or instead of [`bind`].
    pub fn open_with_multi_accept(self: &Arc<Self>) -> Result<(), OpenCloseError> {
        let _ = self
            .config
            .socket_address()
            .ok_or(OpenCloseError::Failed)?;
        if !self.state.is_open() {
            self.state.mark_open()?;
        }
        self.open_stop.store(false, Ordering::SeqCst);

        // Shared circuit for the lifetime of this open (multi PutBytes).
        {
            let mut g = self.circuit.lock().expect("circuit");
            if g.is_none() {
                *g = Some(Secs1Circuit::start_multi(self.circuit_config()));
            }
        }

        let this = Arc::clone(self);
        let handle = thread::spawn(move || {
            while !this.open_stop.load(Ordering::SeqCst) && !this.state.is_closed() {
                if this.listener.lock().expect("listener").is_none() {
                    if this.bind_listener().is_err() {
                        if this.open_stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let wait = this.config.rebind_seconds().get().as_duration();
                        sleep_chunked(wait, &this.open_stop);
                        continue;
                    }
                }

                match this.accept_stream_interruptible() {
                    Ok(stream) => {
                        let this2 = Arc::clone(&this);
                        let worker = thread::spawn(move || {
                            this2.handle_accepted(stream);
                        });
                        this.accept_workers
                            .lock()
                            .expect("accept workers")
                            .push(worker);
                        // Reap finished workers occasionally.
                        this.reap_accept_workers();
                    }
                    Err(_) => {
                        *this.listener.lock().expect("listener") = None;
                        if this.open_stop.load(Ordering::SeqCst) || this.state.is_closed() {
                            break;
                        }
                        let wait = this.config.rebind_seconds().get().as_duration();
                        sleep_chunked(wait, &this.open_stop);
                    }
                }
            }
        });
        *self.open_handle.lock().expect("open handle") = Some(handle);

        // Poller: sync `connected` with multi-circuit channel count.
        let this_c = Arc::clone(self);
        let conn_poll = thread::spawn(move || {
            while !this_c.open_stop.load(Ordering::SeqCst) && !this_c.state.is_closed() {
                let n = this_c
                    .circuit
                    .lock()
                    .expect("circuit")
                    .as_ref()
                    .map(|c| c.channel_count())
                    .unwrap_or(0);
                this_c.connected.set(n > 0);
                thread::sleep(Duration::from_millis(20));
            }
        });
        self.accept_workers
            .lock()
            .expect("accept workers")
            .push(conn_poll);
        Ok(())
    }

    /// Accept one TCP stream (no circuit install) — multi-accept path.
    fn accept_stream_interruptible(&self) -> Result<TcpStream, OpenCloseError> {
        let listener = {
            let g = self.listener.lock().expect("listener");
            g.as_ref()
                .ok_or(OpenCloseError::Failed)?
                .try_clone()
                .map_err(|_| OpenCloseError::Failed)?
        };
        let _ = listener.set_nonblocking(true);
        loop {
            if self.open_stop.load(Ordering::SeqCst) || self.state.is_closed() {
                let _ = listener.set_nonblocking(false);
                return Err(OpenCloseError::Failed);
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = listener.set_nonblocking(false);
                    return Ok(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => {
                    let _ = listener.set_nonblocking(false);
                    return Err(OpenCloseError::Failed);
                }
            }
        }
    }

    /// `HandleAccepted`: attach stream to shared multi-circuit.
    fn handle_accepted(&self, stream: TcpStream) {
        let circuit = {
            let g = self.circuit.lock().expect("circuit");
            match g.as_ref() {
                Some(c) => Arc::clone(c),
                None => return,
            }
        };
        if circuit.attach_channel(stream).is_ok() {
            self.accept_count.fetch_add(1, Ordering::SeqCst);
            self.connected.set(true);
        }
        // Reader thread inside attach_channel lives until peer drop; this worker returns.
    }

    fn reap_accept_workers(&self) {
        let mut g = self.accept_workers.lock().expect("accept workers");
        g.retain(|h| !h.is_finished());
    }

    /// Active TCP channel count on the multi-circuit (0 if sequential/single).
    pub fn channel_count(&self) -> usize {
        self.circuit
            .lock()
            .expect("circuit")
            .as_ref()
            .map(|c| c.channel_count())
            .unwrap_or(0)
    }

    pub fn close(&self) {
        self.open_stop.store(true, Ordering::SeqCst);
        self.teardown_circuit_only();
        // Closing listener unblocks accept.
        *self.listener.lock().expect("listener") = None;
        self.state.mark_closed();
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

    /// Snapshot circuit Arc under lock, then run `f` **without** holding the mutex.
    fn with_circuit<R>(&self, f: impl FnOnce(&Secs1Circuit) -> R) -> Result<R, Secs1Error> {
        let c = {
            let g = self.circuit.lock().expect("circuit");
            g.as_ref()
                .cloned()
                .ok_or(Secs1Error::ChannelShutdown)?
        };
        Ok(f(&c))
    }

    pub fn send(&self, msg: Secs1Message) -> Result<Option<Secs1Message>, Secs1Error> {
        self.with_circuit(|c| c.send(msg))?
    }

    pub fn send_data(
        &self,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        let msg = build_primary(
            self.config.device_id(),
            self.config.is_equip(),
            &self.sys,
            strm,
            func,
            wbit,
            body,
        )?;
        self.send(msg)
    }

    pub fn send_data_reply(
        &self,
        primary: &Secs1Message,
        strm: i32,
        func: i32,
        wbit: bool,
        body: Secs2,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        let msg = build_reply(
            self.config.device_id(),
            self.config.is_equip(),
            primary,
            strm,
            func,
            wbit,
            body,
        )?;
        self.send(msg)
    }

    pub fn recv_primary(&self) -> Result<Secs1Message, Secs1Error> {
        self.with_circuit(|c| c.recv_primary())?
    }

    pub fn poll_primary(
        &self,
        timeout: Duration,
    ) -> Result<Option<Secs1Message>, Secs1Error> {
        self.with_circuit(|c| c.poll_primary(timeout))?
    }
}

impl OpenAndCloseable for Secs1OnTcpIpReceiverCommunicator {
    fn open(&self) -> Result<(), OpenCloseError> {
        Secs1OnTcpIpReceiverCommunicator::open(self)
    }

    fn is_open(&self) -> bool {
        Secs1OnTcpIpReceiverCommunicator::is_open(self)
    }

    fn is_closed(&self) -> bool {
        Secs1OnTcpIpReceiverCommunicator::is_closed(self)
    }

    fn close(&self) {
        Secs1OnTcpIpReceiverCommunicator::close(self)
    }
}

fn sleep_chunked(total: Duration, stop: &AtomicBool) {
    let step = Duration::from_millis(20);
    let mut left = total;
    while left > Duration::ZERO && !stop.load(Ordering::SeqCst) {
        let d = if left > step { step } else { left };
        thread::sleep(d);
        left = left.saturating_sub(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secs1::Secs1Message;
    use crate::secs2::Secs2;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn secs1_ontcpip_config_defaults() {
        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        assert!(cfg.is_master());
        assert!(!cfg.is_equip());
        assert_eq!(cfg.device_id(), 10);
        assert_eq!(cfg.retry(), 3);
        assert!(cfg.is_check_block_device_id());
        assert_eq!(cfg.timeout().t2().get().milli_seconds(), 15_000);
        assert_eq!(cfg.timeout().t4().get().milli_seconds(), 45_000);
    }

    #[test]
    fn secs1_ontcpip_channel_block_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let header = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ];
        let msg = Secs1Message::build_data_message(&header, Secs2::ascii("XY").unwrap()).unwrap();
        let block = msg.to_blocks()[0].clone();

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = Secs1TcpChannel::new(stream, Duration::from_secs(2));
            ch.recv_block().unwrap()
        });

        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        cfg.set_socket_address(addr);
        cfg.timeout().set_t2(2.0);
        let comm = Secs1OnTcpIpCommunicator::new_instance(cfg);
        let mut ch = comm.open_channel().unwrap();
        assert!(comm.is_open());
        ch.send_block(&block).unwrap();

        let got = server.join().unwrap();
        assert!(got.is_valid());
        assert_eq!(got.get_bytes(), block.get_bytes());
    }

    #[test]
    fn secs1_ontcpip_channel_multi_block_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let header = [
            0x00, 0x0A, 0x81, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03,
        ];
        let body = Secs2::ascii("Q".repeat(500)).unwrap();
        let msg = Secs1Message::build_data_message(&header, body).unwrap();
        assert!(msg.to_blocks().len() >= 2);
        let expected = msg.secs2().get_ascii().unwrap();

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ch = Secs1TcpChannel::with_timeouts(
                stream,
                Duration::from_secs(2),
                Duration::from_secs(2),
            );
            ch.recv_message().unwrap()
        });

        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        cfg.set_socket_address(addr);
        cfg.timeout().set_t2(2.0);
        cfg.timeout().set_t4(2.0);
        let comm = Secs1OnTcpIpCommunicator::new_instance(cfg);
        let mut ch = comm.open_channel().unwrap();
        ch.send_message(&msg).unwrap();

        let got = server.join().unwrap();
        assert_eq!(got.secs2().get_ascii().unwrap(), expected);
        assert_eq!(got.get_function(), 5);
    }

    #[test]
    fn secs1_ontcpip_open_circuit_wbit_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let cfg = Secs1OnTcpIpCommunicatorConfig::new();
            cfg.set_is_master(false);
            cfg.timeout().set_t2(2.0);
            cfg.timeout().set_t3(2.0);
            cfg.timeout().set_t4(2.0);
            let comm = Secs1OnTcpIpCommunicator::new_instance(cfg);
            comm.open_with_stream(stream).unwrap();
            let primary = comm.recv_primary().unwrap();
            assert!(primary.wbit());
            let mut h = primary.header10_bytes();
            h[2] = 0x01;
            h[3] = 0x02;
            let rsp =
                Secs1Message::build_data_message(&h, Secs2::ascii("PONG").unwrap()).unwrap();
            comm.send(rsp).unwrap();
            comm.close();
            primary
        });

        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        cfg.set_socket_address(addr);
        cfg.set_is_master(true);
        cfg.timeout().set_t2(2.0);
        cfg.timeout().set_t3(2.0);
        cfg.timeout().set_t4(2.0);
        let client = Secs1OnTcpIpCommunicator::new_instance(cfg);
        client.open().unwrap();
        assert!(client.is_open());

        let header = [0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x77];
        let req =
            Secs1Message::build_data_message(&header, Secs2::ascii("PING").unwrap()).unwrap();
        let reply = client.send(req).unwrap().expect("W-bit reply");
        assert_eq!(reply.secs2().get_ascii().unwrap(), "PONG");

        client.close();
        assert!(client.is_closed());
        let _ = server.join().unwrap();
    }

    #[test]
    fn secs1_client_receiver_send_data_roundtrip() {
        // Receiver binds (equip); client connects (host); send_data W-bit + reply.
        let rcfg = Secs1OnTcpIpReceiverCommunicatorConfig::new();
        rcfg.set_socket_address("127.0.0.1:0".parse().unwrap());
        rcfg.set_is_master(false);
        rcfg.set_is_equip(true);
        rcfg.set_device_id(10).unwrap();
        rcfg.timeout().set_t2(2.0);
        rcfg.timeout().set_t3(2.0);
        rcfg.timeout().set_t4(2.0);
        let receiver = Secs1OnTcpIpReceiverCommunicator::new_instance(rcfg);
        let addr = receiver.bind().unwrap();

        let srv = thread::spawn(move || {
            receiver.accept_one().unwrap();
            let primary = receiver.recv_primary().unwrap();
            assert_eq!(primary.device_id(), 10);
            assert!(primary.wbit());
            assert_eq!(primary.secs2().get_ascii().unwrap(), "PING");
            // Equipment R-bit on reply.
            receiver
                .send_data_reply(&primary, 1, 2, false, Secs2::ascii("PONG").unwrap())
                .unwrap();
            let rsp_hdr = {
                // Wait briefly then close after client gets reply.
                thread::sleep(Duration::from_millis(50));
                receiver.close();
            };
            let _ = rsp_hdr;
            primary
        });

        let ccfg = Secs1OnTcpIpCommunicatorConfig::new();
        ccfg.set_socket_address(addr);
        ccfg.set_is_master(true);
        ccfg.set_is_equip(false);
        ccfg.set_device_id(10).unwrap();
        ccfg.timeout().set_t2(2.0);
        ccfg.timeout().set_t3(2.0);
        ccfg.timeout().set_t4(2.0);
        let client = Secs1OnTcpIpCommunicator::new_instance(ccfg);
        client.open().unwrap();

        let reply = client
            .send_data(1, 1, true, Secs2::ascii("PING").unwrap())
            .unwrap()
            .expect("reply");
        assert_eq!(reply.secs2().get_ascii().unwrap(), "PONG");
        assert!(reply.rbit(), "equip reply should set R-bit");
        assert_eq!(reply.get_function(), 2);

        client.close();
        let _ = srv.join().unwrap();
    }

    #[test]
    fn secs1_device_id_validation() {
        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        assert!(cfg.set_device_id(0).is_ok());
        assert!(cfg.set_device_id(0x7FFF).is_ok());
        assert!(cfg.set_device_id(-1).is_err());
        assert!(cfg.set_device_id(0x8000).is_err());
    }

    #[test]
    fn secs1_ontcpip_reconnect_after_peer_drop() {
        // Server accepts twice; client open_with_reconnect recovers after first drop.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            // First connection: accept, brief exchange, drop.
            let (s1, _) = listener.accept().unwrap();
            let cfg1 = Secs1OnTcpIpCommunicatorConfig::new();
            cfg1.set_is_master(false);
            cfg1.timeout().set_t2(2.0);
            cfg1.timeout().set_t3(2.0);
            cfg1.timeout().set_t4(2.0);
            let c1 = Secs1OnTcpIpCommunicator::new_instance(cfg1);
            c1.open_with_stream(s1).unwrap();
            let p = c1.recv_primary().unwrap();
            assert_eq!(p.secs2().get_ascii().unwrap(), "A1");
            c1.close(); // drop peer

            // Second connection after client reconnect.
            let (s2, _) = listener.accept().unwrap();
            let cfg2 = Secs1OnTcpIpCommunicatorConfig::new();
            cfg2.set_is_master(false);
            cfg2.timeout().set_t2(2.0);
            cfg2.timeout().set_t3(2.0);
            cfg2.timeout().set_t4(2.0);
            let c2 = Secs1OnTcpIpCommunicator::new_instance(cfg2);
            c2.open_with_stream(s2).unwrap();
            let p2 = c2.recv_primary().unwrap();
            assert_eq!(p2.secs2().get_ascii().unwrap(), "A2");
            c2.close();
        });

        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        cfg.set_socket_address(addr);
        cfg.set_is_master(true);
        cfg.set_reconnect_seconds(0.15);
        cfg.timeout().set_t2(2.0);
        cfg.timeout().set_t3(2.0);
        cfg.timeout().set_t4(2.0);
        let client = Arc::new(Secs1OnTcpIpCommunicator::new_instance(cfg));
        client.open_with_reconnect().unwrap();
        assert!(client.wait_until_connected(Duration::from_secs(2)));
        assert_eq!(client.connect_count(), 1);

        client
            .send_data(1, 1, false, Secs2::ascii("A1").unwrap())
            .unwrap();

        // Wait for peer drop to clear connected, then reconnect.
        let mut saw_second = false;
        for _ in 0..100 {
            if client.connect_count() >= 2 && client.is_connected() {
                saw_second = true;
                break;
            }
            thread::sleep(Duration::from_millis(30));
        }
        assert!(saw_second, "expected reconnect, count={}", client.connect_count());

        client
            .send_data(1, 1, false, Secs2::ascii("A2").unwrap())
            .unwrap();

        client.close();
        assert!(client.is_closed());
        server.join().unwrap();
    }

    #[test]
    fn secs1_reconnect_seconds_default() {
        let cfg = Secs1OnTcpIpCommunicatorConfig::new();
        assert_eq!(cfg.reconnect_seconds().get().milli_seconds(), 5_000);
        cfg.set_reconnect_seconds(0.2);
        assert_eq!(cfg.reconnect_seconds().get().milli_seconds(), 200);
    }

    #[test]
    fn secs1_receiver_rebind_sequential_accept() {
        // Receiver open_with_rebind accepts two sequential clients on one bind.
        let rcfg = Secs1OnTcpIpReceiverCommunicatorConfig::new();
        rcfg.set_socket_address("127.0.0.1:0".parse().unwrap());
        rcfg.set_is_master(false);
        rcfg.set_is_equip(true);
        rcfg.set_device_id(10).unwrap();
        rcfg.set_rebind_seconds(0.15);
        rcfg.timeout().set_t2(2.0);
        rcfg.timeout().set_t3(2.0);
        rcfg.timeout().set_t4(2.0);
        let receiver = Arc::new(Secs1OnTcpIpReceiverCommunicator::new_instance(rcfg));
        let addr = receiver.bind().unwrap();
        receiver.open_with_rebind().unwrap();

        // Client 1
        let c1cfg = Secs1OnTcpIpCommunicatorConfig::new();
        c1cfg.set_socket_address(addr);
        c1cfg.set_is_master(true);
        c1cfg.timeout().set_t2(2.0);
        c1cfg.timeout().set_t3(2.0);
        c1cfg.timeout().set_t4(2.0);
        let c1 = Secs1OnTcpIpCommunicator::new_instance(c1cfg);
        c1.open().unwrap();
        assert!(receiver.wait_until_connected(Duration::from_secs(2)));
        c1.send_data(1, 1, false, Secs2::ascii("R1").unwrap())
            .unwrap();
        let p1 = receiver.recv_primary().unwrap();
        assert_eq!(p1.secs2().get_ascii().unwrap(), "R1");
        assert_eq!(receiver.accept_count(), 1);
        c1.close();

        // Wait for first circuit teardown.
        for _ in 0..80 {
            if !receiver.is_connected() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        // Client 2 on same bind.
        let c2cfg = Secs1OnTcpIpCommunicatorConfig::new();
        c2cfg.set_socket_address(addr);
        c2cfg.set_is_master(true);
        c2cfg.timeout().set_t2(2.0);
        c2cfg.timeout().set_t3(2.0);
        c2cfg.timeout().set_t4(2.0);
        let c2 = Secs1OnTcpIpCommunicator::new_instance(c2cfg);
        c2.open().unwrap();
        assert!(receiver.wait_until_connected(Duration::from_secs(2)));
        c2.send_data(1, 1, false, Secs2::ascii("R2").unwrap())
            .unwrap();
        let p2 = receiver.recv_primary().unwrap();
        assert_eq!(p2.secs2().get_ascii().unwrap(), "R2");
        assert!(
            receiver.accept_count() >= 2,
            "accepts={}",
            receiver.accept_count()
        );
        c2.close();

        receiver.close();
        assert!(receiver.is_closed());
    }

    #[test]
    fn secs1_rebind_seconds_default() {
        let cfg = Secs1OnTcpIpReceiverCommunicatorConfig::new();
        assert_eq!(cfg.rebind_seconds().get().milli_seconds(), 5_000);
    }

    /// Concurrent multi-accept: two TCP channels attached at once; sequential SECS-I send.
    ///
    /// (Byte queue is shared — concurrent SECS-I frames from two peers would interleave;
    /// C# same model. We only assert multi-attach + ordered single-sender traffic.)
    #[test]
    fn secs1_receiver_multi_accept_two_clients() {
        let rcfg = Secs1OnTcpIpReceiverCommunicatorConfig::new();
        rcfg.set_socket_address("127.0.0.1:0".parse().unwrap());
        rcfg.set_is_master(false);
        rcfg.set_is_equip(true);
        rcfg.set_device_id(10).unwrap();
        rcfg.set_rebind_seconds(0.15);
        rcfg.timeout().set_t2(2.0);
        rcfg.timeout().set_t3(2.0);
        rcfg.timeout().set_t4(2.0);
        let receiver = Arc::new(Secs1OnTcpIpReceiverCommunicator::new_instance(rcfg));
        let addr = receiver.bind().unwrap();
        receiver.open_with_multi_accept().unwrap();

        let open_client = || {
            let ccfg = Secs1OnTcpIpCommunicatorConfig::new();
            ccfg.set_socket_address(addr);
            ccfg.set_is_master(true);
            ccfg.set_device_id(10).unwrap();
            ccfg.timeout().set_t2(2.0);
            ccfg.timeout().set_t3(2.0);
            ccfg.timeout().set_t4(2.0);
            let c = Secs1OnTcpIpCommunicator::new_instance(ccfg);
            c.open().unwrap();
            c
        };

        let c1 = open_client();
        assert!(receiver.wait_until_connected(Duration::from_secs(2)));
        let c2 = open_client();

        // Both channels attached concurrently (second without dropping first).
        let mut saw_two = false;
        for _ in 0..50 {
            if receiver.channel_count() >= 2 {
                saw_two = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            saw_two,
            "expected 2 channels, got {}",
            receiver.channel_count()
        );
        assert!(receiver.accept_count() >= 2);

        // Traffic one-at-a-time on first connected peer.
        c1.send_data(1, 1, false, Secs2::ascii("C1").unwrap())
            .unwrap();
        let p1 = receiver
            .poll_primary(Duration::from_secs(2))
            .unwrap()
            .expect("C1 primary");
        assert_eq!(p1.secs2().get_ascii().unwrap(), "C1");

        // Drop first channel; send should move to remaining channel (GetChannel → [0]).
        c1.close();
        for _ in 0..50 {
            if receiver.channel_count() == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(receiver.channel_count(), 1);

        c2.send_data(1, 1, false, Secs2::ascii("C2").unwrap())
            .unwrap();
        let p2 = receiver
            .poll_primary(Duration::from_secs(2))
            .unwrap()
            .expect("C2 primary");
        assert_eq!(p2.secs2().get_ascii().unwrap(), "C2");

        c2.close();
        receiver.close();
        assert!(receiver.is_closed());
    }
}
