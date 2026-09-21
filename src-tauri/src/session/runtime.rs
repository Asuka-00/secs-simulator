//! Per-session HSMS-SS runtime: config build, open/close, state + catalog auto-reply.
//! 单会话 HSMS-SS 运行时：配置、打开/关闭、状态与目录自动应答。

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use secs4rs::hsms::{HsmsCommunicateState, HsmsConnectionMode, HsmsMessage};
use secs4rs::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::catalog::{
    new_shared_catalog, try_catalog_auto_reply, MessageCatalog, SharedCatalog,
};
use crate::error::{AppError, AppResult};
use crate::flow::FlowRuntime;
use crate::session::config::{ConnectionMode, Role, SessionConfig};
use crate::session::log::{LogDirection, LogEntry};
use crate::session::SharedSessionManager;

/// UI / event channel name (single channel, payload carries sessionId).
pub const SESSION_EVENT: &str = "session-event";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub session_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hsms: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<LogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sx_fy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl SessionEvent {
    fn base(session_id: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: event_type.into(),
            open: None,
            hsms: None,
            message: None,
            entry: None,
            rule_id: None,
            sx_fy: None,
            flow_id: None,
            node_id: None,
        }
    }

    pub fn state(session_id: impl Into<String>, open: bool, hsms: impl Into<String>) -> Self {
        Self {
            open: Some(open),
            hsms: Some(hsms.into()),
            ..Self::base(session_id, "state")
        }
    }

    pub fn error(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::base(session_id, "error")
        }
    }

    pub fn log(session_id: impl Into<String>, entry: LogEntry) -> Self {
        Self {
            entry: Some(entry),
            ..Self::base(session_id, "log")
        }
    }

    pub fn rule_hit(
        session_id: impl Into<String>,
        rule_id: impl Into<String>,
        sx_fy: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: Some(rule_id.into()),
            sx_fy: Some(sx_fy.into()),
            ..Self::base(session_id, "rule_hit")
        }
    }

    pub fn send_done(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::base(session_id, "send_done")
        }
    }

    pub fn flow_progress(
        session_id: impl Into<String>,
        flow_id: impl Into<String>,
        node_id: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            flow_id: Some(flow_id.into()),
            node_id: Some(node_id.into()),
            message: Some(message.into()),
            ..Self::base(session_id, "flow_progress")
        }
    }

    pub fn flow_done(
        session_id: impl Into<String>,
        flow_id: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            flow_id: Some(flow_id.into()),
            message: Some(message.into()),
            ..Self::base(session_id, "flow_done")
        }
    }
}

pub fn emit_session_event(app: &Option<AppHandle>, event: SessionEvent) {
    if let Some(app) = app {
        let _ = app.emit(SESSION_EVENT, event);
    }
}

/// Append log entry to session buffer and emit to UI.
pub fn push_log(
    manager: &SharedSessionManager,
    app: &Option<AppHandle>,
    session_id: &str,
    entry: LogEntry,
) {
    if let Ok(mut g) = manager.lock() {
        if let Some(s) = g.session_mut(session_id) {
            s.logs.push(entry.clone());
        } else {
            return;
        }
    } else {
        return;
    }
    emit_session_event(app, SessionEvent::log(session_id, entry));
}

pub fn hsms_state_label(state: HsmsCommunicateState) -> &'static str {
    match state {
        HsmsCommunicateState::NotConnected => "NotConnected",
        HsmsCommunicateState::NotSelected => "NotSelected",
        HsmsCommunicateState::Selected => "Selected",
    }
}

/// Active connects to `cfg.ip` (IP or hostname). Passive binds all interfaces.
pub fn resolve_hsms_socket_addr(cfg: &SessionConfig) -> AppResult<SocketAddr> {
    match cfg.mode {
        ConnectionMode::Active => resolve_active_addr(&cfg.ip, cfg.port),
        ConnectionMode::Passive => {
            let listen = if looks_like_ipv6(&cfg.ip) {
                "::"
            } else {
                "0.0.0.0"
            };
            parse_ip_port(listen, cfg.port)
        }
    }
}

/// Default-route IPv4, for "peer should connect here" hints.
pub fn local_outbound_ipv4() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("1.1.1.1:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn looks_like_ipv6(ip: &str) -> bool {
    ip.contains(':')
}

fn parse_ip_port(ip: &str, port: u16) -> AppResult<SocketAddr> {
    let spec = if ip.contains(':') && !ip.starts_with('[') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    };
    spec.parse()
        .map_err(|e| AppError::Message(format!("invalid ip/port: {e}")))
}

fn resolve_active_addr(host: &str, port: u16) -> AppResult<SocketAddr> {
    let host = host.trim();
    if host.is_empty() {
        return Err(AppError::Message("active ip is empty".into()));
    }
    if let Ok(addr) = parse_ip_port(host, port) {
        if addr.ip().is_unspecified() {
            return Err(AppError::Message(
                "Active cannot connect to 0.0.0.0/::; set the peer LAN IP".into(),
            ));
        }
        return Ok(addr);
    }
    let spec = format!("{host}:{port}");
    spec.to_socket_addrs()
        .map_err(|e| AppError::Message(format!("cannot resolve {spec}: {e}")))?
        .next()
        .ok_or_else(|| AppError::Message(format!("cannot resolve {spec}")))
}

/// Build secs4rs HSMS-SS config from simulator session config.
pub fn build_hsms_config(cfg: &SessionConfig) -> AppResult<HsmsSsCommunicatorConfig> {
    let c = HsmsSsCommunicatorConfig::new();

    c.set_session_id(cfg.session_id)
        .map_err(|_| AppError::Message(format!("invalid session id: {}", cfg.session_id)))?;

    c.set_connection_mode(match cfg.mode {
        ConnectionMode::Active => HsmsConnectionMode::Active,
        ConnectionMode::Passive => HsmsConnectionMode::Passive,
    });

    c.set_socket_address(resolve_hsms_socket_addr(cfg)?);

    c.timeout().set_t3(cfg.t3);
    c.timeout().set_t5(cfg.t5);
    c.timeout().set_t6(cfg.t6);
    c.timeout().set_t7(cfg.t7);
    c.timeout().set_t8(cfg.t8);

    c.set_is_equip(matches!(cfg.role, Role::Equipment));

    if cfg.linktest_enabled {
        c.linktest(cfg.linktest_seconds);
    } else {
        c.not_linktest();
    }

    if cfg.rebind_if_passive {
        c.rebind_if_passive(cfg.t5.max(1.0));
    } else {
        c.not_rebind_if_passive();
    }

    Ok(c)
}

fn attach_message_loggers(
    comm: &HsmsSsCommunicator,
    session_id: String,
    manager: SharedSessionManager,
    app: Option<AppHandle>,
) {
    {
        let sid = session_id.clone();
        let mgr = Arc::clone(&manager);
        let app_h = app.clone();
        comm.pass_through().add_sended(move |msg: &HsmsMessage| {
            let entry = LogEntry::from_hsms(msg, LogDirection::Tx);
            push_log(&mgr, &app_h, &sid, entry);
        });
    }
    {
        let sid = session_id.clone();
        let mgr = Arc::clone(&manager);
        let app_h = app.clone();
        comm.pass_through().add_receive(move |msg: &HsmsMessage| {
            let entry = LogEntry::from_hsms(msg, LogDirection::Rx);
            push_log(&mgr, &app_h, &sid, entry);
        });
    }
}

fn attach_auto_reply(
    comm: &Arc<HsmsSsCommunicator>,
    session_id: String,
    role: Role,
    catalog: SharedCatalog,
    pending_primary: Arc<Mutex<Option<HsmsMessage>>>,
    manager: SharedSessionManager,
    app: Option<AppHandle>,
    flow_rt: Option<FlowRuntime>,
) {
    let c = Arc::clone(comm);
    let sid = session_id;
    comm.add_hsms_message_receive_listener(move |msg: &HsmsMessage| {
        if !msg.is_data_message() {
            return;
        }
        let sx = format!("S{}F{}", msg.get_stream(), msg.get_function());
        let cat = match catalog.lock() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        if let Some(note) = try_catalog_auto_reply(&c, msg, &cat, &role) {
            push_log(
                &manager,
                &app,
                &sid,
                LogEntry::system(format!("{note} for {sx}")),
            );
            emit_session_event(&app, SessionEvent::rule_hit(&sid, "catalog", sx));
        } else if msg.wbit() && msg.get_function() % 2 == 1 {
            if let Ok(mut g) = pending_primary.lock() {
                *g = Some(msg.clone());
            }
        }
        if let Some(rt) = &flow_rt {
            rt.on_inbound(msg);
        }
    });
}

/// Live HSMS session handle.
pub struct SessionRuntime {
    pub comm: Arc<HsmsSsCommunicator>,
    pub catalog: SharedCatalog,
    pub role: Role,
    /// Last inbound W-bit primary not consumed by AutoReply (for manual secondary).
    pub pending_primary: Arc<Mutex<Option<HsmsMessage>>>,
}

impl SessionRuntime {
    /// Start background open. `app` may be `None` in unit tests (no UI emit).
    pub fn start(
        session_id: String,
        cfg: &SessionConfig,
        catalog: MessageCatalog,
        manager: SharedSessionManager,
        app: Option<AppHandle>,
        flow_rt: Option<FlowRuntime>,
    ) -> AppResult<Self> {
        let hsms_cfg = build_hsms_config(cfg)?;
        let comm = Arc::new(HsmsSsCommunicator::new_instance(hsms_cfg));
        let catalog = new_shared_catalog(catalog);
        let role = cfg.role.clone();
        let pending_primary = Arc::new(Mutex::new(None));

        attach_message_loggers(&comm, session_id.clone(), Arc::clone(&manager), app.clone());
        attach_auto_reply(
            &comm,
            session_id.clone(),
            role.clone(),
            Arc::clone(&catalog),
            Arc::clone(&pending_primary),
            Arc::clone(&manager),
            app.clone(),
            flow_rt.clone(),
        );

        let sock = resolve_hsms_socket_addr(cfg)?;
        let lan = local_outbound_ipv4();
        let extra = match cfg.mode {
            ConnectionMode::Active if sock.ip().is_loopback() => {
                " (loopback: remote hosts unreachable; set peer LAN IP)"
            }
            ConnectionMode::Passive => "",
            _ => "",
        };
        let peer_hint = match (&cfg.mode, &lan) {
            (ConnectionMode::Passive, Some(ip)) => format!("; peer Active should connect {ip}:{}", cfg.port),
            _ => String::new(),
        };
        push_log(
            &manager,
            &app,
            &session_id,
            LogEntry::system(format!(
                "open {} {} sessionId={} equip={}{extra}{peer_hint}",
                match cfg.mode {
                    ConnectionMode::Active => "Active connect",
                    ConnectionMode::Passive => "Passive listen",
                },
                sock,
                cfg.session_id,
                matches!(cfg.role, Role::Equipment)
            )),
        );

        {
            let sid = session_id.clone();
            let mgr = Arc::clone(&manager);
            let app_h = app.clone();
            let fr = flow_rt.clone();
            comm.communicate_state_prop().add_change_listener(move |st| {
                let label = hsms_state_label(*st).to_string();
                let open_flag = {
                    if let Ok(mut g) = mgr.lock() {
                        if let Some(s) = g.session_mut(&sid) {
                            s.hsms_state = label.clone();
                            s.open
                        } else {
                            return;
                        }
                    } else {
                        return;
                    }
                };
                emit_session_event(&app_h, SessionEvent::state(&sid, open_flag, &label));
                if let Some(rt) = &fr {
                    rt.on_hsms(&label);
                }
            });
        }

        match cfg.mode {
            ConnectionMode::Active => {
                comm.open_active_with_t5_retry()
                    .map_err(|e| AppError::Message(format!("open active failed: {e}")))?;
            }
            ConnectionMode::Passive => {
                comm.open_passive_with_rebind()
                    .map_err(|e| AppError::Message(format!("open passive failed: {e}")))?;
            }
        }

        let label = hsms_state_label(comm.hsms_communicate_state()).to_string();
        emit_session_event(&app, SessionEvent::state(&session_id, true, &label));

        {
            let sid = session_id.clone();
            let mgr = Arc::clone(&manager);
            let app_h = app.clone();
            let c = Arc::clone(&comm);
            let fr = flow_rt.clone();
            thread::spawn(move || {
                let mut last = String::new();
                let mut last_err = String::new();
                loop {
                    let still = {
                        let g = match mgr.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        matches!(g.session_ref(&sid), Some(s) if s.open)
                    };
                    if !still {
                        break;
                    }

                    if let Some(err) = c.take_last_open_error() {
                        if err != last_err {
                            last_err = err.clone();
                            push_log(
                                &mgr,
                                &app_h,
                                &sid,
                                LogEntry::system(format!("open failed: {err}")),
                            );
                        }
                    }

                    let label = hsms_state_label(c.hsms_communicate_state()).to_string();
                    if label != last {
                        last = label.clone();
                        if let Ok(mut g) = mgr.lock() {
                            if let Some(s) = g.session_mut(&sid) {
                                s.hsms_state = label.clone();
                            }
                        }
                        emit_session_event(&app_h, SessionEvent::state(&sid, true, &label));
                        if let Some(rt) = &fr {
                            rt.on_hsms(&label);
                        }
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
        }

        Ok(Self {
            comm,
            catalog,
            role,
            pending_primary,
        })
    }

    /// If `function` is the secondary for the remembered primary, take it.
    pub fn take_pending_reply(&self, stream: i32, function: i32) -> Option<HsmsMessage> {
        let mut g = self.pending_primary.lock().ok()?;
        let pri = g.as_ref()?;
        if pri.get_stream() == stream && pri.get_function() + 1 == function {
            g.take()
        } else {
            None
        }
    }

    pub fn close(&self) {
        self.comm.close();
    }

    pub fn hsms_state(&self) -> String {
        hsms_state_label(self.comm.hsms_communicate_state()).to_string()
    }

    pub fn send_data(
        &self,
        stream: i32,
        function: i32,
        wbit: bool,
        body: secs4rs::secs2::Secs2,
    ) -> AppResult<Option<HsmsMessage>> {
        self.comm
            .send_data(stream, function, wbit, body)
            .map_err(|e| AppError::Message(format!("send_data failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{ConnectionMode, Role, SessionConfig};

    fn cfg(mode: ConnectionMode, ip: &str, port: u16) -> SessionConfig {
        SessionConfig {
            name: "t".into(),
            role: Role::Equipment,
            mode,
            ip: ip.into(),
            port,
            session_id: 10,
            ..SessionConfig::default()
        }
    }

    #[test]
    fn build_hsms_passive_binds_unspecified_v4() {
        let c = build_hsms_config(&cfg(ConnectionMode::Passive, "127.0.0.1", 5000)).unwrap();
        let addr = c.socket_address().expect("addr");
        assert!(addr.ip().is_unspecified(), "passive must bind 0.0.0.0, got {addr}");
        assert!(addr.is_ipv4());
        assert_eq!(addr.port(), 5000);
    }

    #[test]
    fn build_hsms_passive_ipv6_binds_unspecified() {
        let c = build_hsms_config(&cfg(ConnectionMode::Passive, "::1", 5000)).unwrap();
        let addr = c.socket_address().expect("addr");
        assert!(addr.ip().is_unspecified(), "passive IPv6 must bind [::], got {addr}");
        assert!(addr.is_ipv6());
        assert_eq!(addr.port(), 5000);
    }

    #[test]
    fn build_hsms_active_keeps_configured_ip() {
        let c = build_hsms_config(&cfg(ConnectionMode::Active, "192.168.1.10", 5000)).unwrap();
        let addr = c.socket_address().expect("addr");
        assert_eq!(addr.to_string(), "192.168.1.10:5000");
    }

    #[test]
    fn build_hsms_active_rejects_unspecified() {
        match build_hsms_config(&cfg(ConnectionMode::Active, "0.0.0.0", 5000)) {
            Ok(_) => panic!("Active 0.0.0.0 must be rejected"),
            Err(err) => assert!(
                err.to_string().contains("0.0.0.0"),
                "expected unspecified reject, got {err}"
            ),
        }
    }

    #[test]
    fn build_hsms_active_resolves_localhost() {
        let addr = resolve_hsms_socket_addr(&cfg(ConnectionMode::Active, "localhost", 5000)).unwrap();
        assert!(addr.ip().is_loopback(), "localhost should be loopback, got {addr}");
        assert_eq!(addr.port(), 5000);
    }
}
