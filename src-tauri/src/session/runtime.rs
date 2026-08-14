//! Per-session HSMS-SS runtime: config build, open/close, state + catalog auto-reply.
//! 单会话 HSMS-SS 运行时：配置、打开/关闭、状态与目录自动应答。

use std::net::SocketAddr;
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
}

impl SessionEvent {
    pub fn state(session_id: impl Into<String>, open: bool, hsms: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: "state".into(),
            open: Some(open),
            hsms: Some(hsms.into()),
            message: None,
            entry: None,
            rule_id: None,
            sx_fy: None,
        }
    }

    pub fn error(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: "error".into(),
            open: None,
            hsms: None,
            message: Some(message.into()),
            entry: None,
            rule_id: None,
            sx_fy: None,
        }
    }

    pub fn log(session_id: impl Into<String>, entry: LogEntry) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: "log".into(),
            open: None,
            hsms: None,
            message: None,
            entry: Some(entry),
            rule_id: None,
            sx_fy: None,
        }
    }

    pub fn rule_hit(
        session_id: impl Into<String>,
        rule_id: impl Into<String>,
        sx_fy: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: "rule_hit".into(),
            open: None,
            hsms: None,
            message: None,
            entry: None,
            rule_id: Some(rule_id.into()),
            sx_fy: Some(sx_fy.into()),
        }
    }

    pub fn send_done(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            event_type: "send_done".into(),
            open: None,
            hsms: None,
            message: Some(message.into()),
            entry: None,
            rule_id: None,
            sx_fy: None,
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

/// Build secs4rs HSMS-SS config from simulator session config.
pub fn build_hsms_config(cfg: &SessionConfig) -> AppResult<HsmsSsCommunicatorConfig> {
    let c = HsmsSsCommunicatorConfig::new();

    c.set_session_id(cfg.session_id)
        .map_err(|_| AppError::Message(format!("invalid session id: {}", cfg.session_id)))?;

    c.set_connection_mode(match cfg.mode {
        ConnectionMode::Active => HsmsConnectionMode::Active,
        ConnectionMode::Passive => HsmsConnectionMode::Passive,
    });

    let addr: SocketAddr = format!("{}:{}", cfg.ip, cfg.port)
        .parse()
        .map_err(|e| AppError::Message(format!("invalid ip/port: {e}")))?;
    c.set_socket_address(addr);

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
            return;
        }
        // Remember W-bit primary so the user can send the secondary as a real reply.
        if msg.wbit() && msg.get_function() % 2 == 1 {
            if let Ok(mut g) = pending_primary.lock() {
                *g = Some(msg.clone());
            }
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
        );

        push_log(
            &manager,
            &app,
            &session_id,
            LogEntry::system(format!(
                "open {} {}:{} sessionId={} equip={}",
                match cfg.mode {
                    ConnectionMode::Active => "Active",
                    ConnectionMode::Passive => "Passive",
                },
                cfg.ip,
                cfg.port,
                cfg.session_id,
                matches!(cfg.role, Role::Equipment)
            )),
        );

        {
            let sid = session_id.clone();
            let mgr = Arc::clone(&manager);
            let app_h = app.clone();
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
                emit_session_event(&app_h, SessionEvent::state(&sid, open_flag, label));
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
            thread::spawn(move || {
                let mut last = String::new();
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

                    let label = hsms_state_label(c.hsms_communicate_state()).to_string();
                    if label != last {
                        last = label.clone();
                        if let Ok(mut g) = mgr.lock() {
                            if let Some(s) = g.session_mut(&sid) {
                                s.hsms_state = label.clone();
                            }
                        }
                        emit_session_event(&app_h, SessionEvent::state(&sid, true, label));
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
