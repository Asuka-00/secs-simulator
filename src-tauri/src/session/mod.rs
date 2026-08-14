//! Multi-session manager.

pub mod config;
pub mod log;
pub mod runtime;
pub mod templates;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use uuid::Uuid;

use crate::catalog::{MessageCatalog, PrefabMessage};
use crate::error::{AppError, AppResult};
use crate::persistence::{
    AppPersistState, AppSettings, ScenarioFile, SessionSnapshot,
};
use config::SessionConfig;
use log::{LogEntry, SessionLog};
use runtime::{emit_session_event, push_log, SessionEvent, SessionRuntime};

/// Frontend-facing session summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub open: bool,
    pub hsms_state: String,
}

/// In-memory session handle.
pub struct Session {
    pub id: String,
    pub config: SessionConfig,
    pub open: bool,
    pub hsms_state: String,
    pub logs: SessionLog,
    pub catalog: MessageCatalog,
    runtime: Option<SessionRuntime>,
}

impl Session {
    fn summary(&self) -> SessionSummary {
        SessionSummary {
            id: self.id.clone(),
            name: self.config.name.clone(),
            open: self.open,
            hsms_state: self.hsms_state.clone(),
        }
    }
}

/// Process-wide multi-session manager.
#[derive(Default)]
pub struct SessionManager {
    sessions: HashMap<String, Session>,
    settings: AppSettings,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            settings: AppSettings::default(),
        }
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    /// Snapshot closed+open configs (runtime not included).
    pub fn snapshot(&self) -> AppPersistState {
        let mut sessions: Vec<SessionSnapshot> = self
            .sessions
            .values()
            .map(|s| SessionSnapshot {
                id: s.id.clone(),
                config: s.config.clone(),
                catalog: s.catalog.clone(),
            })
            .collect();
        sessions.sort_by(|a, b| a.config.name.cmp(&b.config.name).then(a.id.cmp(&b.id)));
        AppPersistState {
            version: 1,
            settings: self.settings.clone(),
            sessions,
        }
    }

    pub fn to_scenario(&self, name: impl Into<String>) -> ScenarioFile {
        let snap = self.snapshot();
        ScenarioFile {
            version: 1,
            name: name.into(),
            sessions: snap.sessions,
        }
    }

    /// Replace all sessions from snapshot (must have no open sessions).
    pub fn restore_snapshot(&mut self, state: AppPersistState) -> AppResult<()> {
        if self.sessions.values().any(|s| s.open) {
            return Err(AppError::Message(
                "close all sessions before restore/import".into(),
            ));
        }
        self.settings = state.settings;
        self.sessions.clear();
        for snap in state.sessions {
            let session = Session {
                id: snap.id.clone(),
                config: snap.config,
                open: false,
                hsms_state: "NotConnected".into(),
                logs: SessionLog::new(self.settings.log_capacity.max(1)),
                catalog: snap.catalog,
                runtime: None,
            };
            self.sessions.insert(snap.id, session);
        }
        Ok(())
    }

    pub fn import_scenario(&mut self, scenario: ScenarioFile) -> AppResult<usize> {
        let n = scenario.sessions.len();
        self.restore_snapshot(AppPersistState {
            version: scenario.version,
            settings: self.settings.clone(),
            sessions: scenario.sessions,
        })?;
        Ok(n)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        let mut out: Vec<_> = self.sessions.values().map(Session::summary).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out
    }

    pub fn session_ref(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn create(&mut self, config: Option<SessionConfig>) -> SessionSummary {
        let mut config = config.unwrap_or_default();
        if config.name.trim().is_empty() {
            config.name = format!("Session {}", self.sessions.len() + 1);
        }
        let id = Uuid::new_v4().to_string();
        let capacity = self.settings.log_capacity.max(1);
        let session = Session {
            id: id.clone(),
            config,
            open: false,
            hsms_state: "NotConnected".into(),
            logs: SessionLog::new(capacity),
            catalog: MessageCatalog::default(),
            runtime: None,
        };
        let summary = session.summary();
        self.sessions.insert(id, session);
        summary
    }

    pub fn remove(&mut self, id: &str) -> AppResult<()> {
        let Some(session) = self.sessions.get(id) else {
            return Err(AppError::Message(format!("session not found: {id}")));
        };
        if session.open {
            return Err(AppError::Message(
                "session is open; close it before remove".into(),
            ));
        }
        self.sessions.remove(id);
        Ok(())
    }

    pub fn update_config(&mut self, id: &str, config: SessionConfig) -> AppResult<SessionSummary> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        if session.open {
            return Err(AppError::Message(
                "cannot update full config while session is open".into(),
            ));
        }
        session.config = config;
        Ok(session.summary())
    }

    pub fn get_config(&self, id: &str) -> AppResult<SessionConfig> {
        self.sessions
            .get(id)
            .map(|s| s.config.clone())
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))
    }

    pub fn get_summary(&self, id: &str) -> AppResult<SessionSummary> {
        self.sessions
            .get(id)
            .map(Session::summary)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))
    }

    pub fn get_catalog(&self, id: &str) -> AppResult<MessageCatalog> {
        self.sessions
            .get(id)
            .map(|s| s.catalog.clone())
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))
    }

    pub fn set_catalog(&mut self, id: &str, catalog: MessageCatalog) -> AppResult<()> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        session.catalog = catalog.clone();
        if let Some(rt) = session.runtime.as_ref() {
            if let Ok(mut c) = rt.catalog.lock() {
                *c = catalog;
            }
        }
        Ok(())
    }

    pub fn upsert_message(&mut self, id: &str, mut msg: PrefabMessage) -> AppResult<MessageCatalog> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        msg.sync_body_sml_from_tree();
        session.catalog.upsert(msg);
        let cat = session.catalog.clone();
        if let Some(rt) = session.runtime.as_ref() {
            if let Ok(mut c) = rt.catalog.lock() {
                *c = cat.clone();
            }
        }
        Ok(cat)
    }

    pub fn remove_message(&mut self, id: &str, message_id: &str) -> AppResult<MessageCatalog> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        session.catalog.remove(message_id);
        let cat = session.catalog.clone();
        if let Some(rt) = session.runtime.as_ref() {
            if let Ok(mut c) = rt.catalog.lock() {
                *c = cat.clone();
            }
        }
        Ok(cat)
    }

    pub fn import_smd(
        &mut self,
        id: &str,
        xml: &str,
        source: &str,
    ) -> AppResult<(usize, MessageCatalog)> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        let n = session.catalog.import_smd(xml, source)?;
        let cat = session.catalog.clone();
        if let Some(rt) = session.runtime.as_ref() {
            if let Ok(mut c) = rt.catalog.lock() {
                *c = cat.clone();
            }
        }
        Ok((n, cat))
    }

    pub fn get_logs(&self, id: &str) -> AppResult<Vec<LogEntry>> {
        self.sessions
            .get(id)
            .map(|s| s.logs.entries())
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))
    }

    pub fn clear_logs(&mut self, id: &str) -> AppResult<()> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
        session.logs.clear();
        Ok(())
    }

    /// Open HSMS for session (background select/retry). Emits `session-event` when `app` set.
    pub fn open_session(
        manager: &SharedSessionManager,
        id: &str,
        app: Option<AppHandle>,
    ) -> AppResult<SessionSummary> {
        let (config, catalog) = {
            let mut guard = manager
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            let session = guard
                .sessions
                .get_mut(id)
                .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
            if session.open {
                return Err(AppError::Message("session already open".into()));
            }
            (session.config.clone(), session.catalog.clone())
        };

        let runtime = match SessionRuntime::start(
            id.to_string(),
            &config,
            catalog,
            Arc::clone(manager),
            app.clone(),
        ) {
            Ok(rt) => rt,
            Err(e) => {
                emit_session_event(&app, SessionEvent::error(id, e.to_string()));
                return Err(e);
            }
        };

        let summary = {
            let mut guard = manager
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            let session = guard
                .sessions
                .get_mut(id)
                .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
            session.open = true;
            session.hsms_state = runtime.hsms_state();
            session.runtime = Some(runtime);
            session.summary()
        };

        emit_session_event(
            &app,
            SessionEvent::state(id, true, &summary.hsms_state),
        );
        Ok(summary)
    }

    /// Close HSMS for session.
    pub fn close_session(
        manager: &SharedSessionManager,
        id: &str,
        app: Option<AppHandle>,
    ) -> AppResult<SessionSummary> {
        let runtime = {
            let mut guard = manager
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            let session = guard
                .sessions
                .get_mut(id)
                .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
            if !session.open {
                return Err(AppError::Message("session is not open".into()));
            }
            session.open = false;
            session.runtime.take()
        };

        if let Some(rt) = runtime {
            rt.close();
        }

        push_log(
            manager,
            &app,
            id,
            LogEntry::system("closed"),
        );

        let summary = {
            let mut guard = manager
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            let session = guard
                .sessions
                .get_mut(id)
                .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
            session.open = false;
            session.hsms_state = "NotConnected".into();
            session.summary()
        };

        emit_session_event(&app, SessionEvent::state(id, false, "NotConnected"));
        Ok(summary)
    }

    /// Send DATA via live runtime.
    ///
    /// Even-function sends reuse the last unmatched inbound W-bit primary
    /// (`send_data_reply`) so a manual S6F12 completes the peer's T3 wait.
    ///
    /// Does **not** hold the manager lock across wire I/O (pass-through loggers re-enter the lock).
    pub fn send_data(
        manager: &SharedSessionManager,
        id: &str,
        stream: i32,
        function: i32,
        wbit: bool,
        body: secs4rs::secs2::Secs2,
    ) -> AppResult<Option<secs4rs::hsms::HsmsMessage>> {
        let (comm, reply_to) = {
            let guard = manager
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            let session = guard
                .sessions
                .get(id)
                .ok_or_else(|| AppError::Message(format!("session not found: {id}")))?;
            if !session.open {
                return Err(AppError::Message("session is not open".into()));
            }
            let rt = session
                .runtime
                .as_ref()
                .ok_or_else(|| AppError::Message("session is not open".into()))?;
            if session.hsms_state != "Selected" {
                // Best-effort: still try send; communicator will error if not selected.
            }
            let reply_to = if function % 2 == 0 {
                rt.take_pending_reply(stream, function)
            } else {
                None
            };
            (Arc::clone(&rt.comm), reply_to)
        };
        if let Some(primary) = reply_to {
            comm.send_data_reply(&primary, stream, function, wbit, body)
                .map_err(|e| AppError::Message(format!("send_data_reply failed: {e}")))?;
            return Ok(None);
        }
        comm.send_data(stream, function, wbit, body)
            .map_err(|e| AppError::Message(format!("send_data failed: {e}")))
    }

    /// Like [`send_data`], but a W-bit primary returns immediately and waits T3
    /// on a background thread so the UI command pool stays free for a manual reply.
    pub fn send_data_ui(
        manager: &SharedSessionManager,
        id: &str,
        stream: i32,
        function: i32,
        wbit: bool,
        body: secs4rs::secs2::Secs2,
        app: Option<tauri::AppHandle>,
    ) -> AppResult<Option<secs4rs::hsms::HsmsMessage>> {
        let wait_async = wbit && function % 2 == 1 && app.is_some();
        if !wait_async {
            return Self::send_data(manager, id, stream, function, wbit, body);
        }
        let mgr = Arc::clone(manager);
        let sid = id.to_string();
        thread::spawn(move || {
            match Self::send_data(&mgr, &sid, stream, function, true, body) {
                Ok(Some(reply)) => {
                    let info = crate::sml_bridge::reply_info(&reply);
                    emit_session_event(
                        &app,
                        SessionEvent::send_done(
                            &sid,
                            format!("S{stream}F{function} W → {}", info.summary),
                        ),
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    push_log(&mgr, &app, &sid, LogEntry::system(e.to_string()));
                    emit_session_event(&app, SessionEvent::error(&sid, e.to_string()));
                }
            }
        });
        Ok(None)
    }

    /// Parse SML and send as HSMS DATA primary. If W-bit, waits for reply (T3).
    pub fn send_sml(
        manager: &SharedSessionManager,
        id: &str,
        sml: &str,
    ) -> AppResult<crate::sml_bridge::SendSmlResult> {
        use crate::sml_bridge::{reply_info, parse_sml, SendSmlResult};

        let msg = parse_sml(sml)?;
        let stream = msg.get_stream();
        let function = msg.get_function();
        let wbit = msg.wbit();
        let body = msg.secs2().clone();
        let w = if wbit { " W" } else { "" };
        let summary = format!("S{stream}F{function}{w}");

        let reply_msg = Self::send_data(manager, id, stream, function, wbit, body)?;
        let reply = reply_msg.as_ref().map(reply_info);

        Ok(SendSmlResult {
            stream,
            function,
            wbit,
            summary,
            reply,
            waiting: false,
        })
    }
}

/// Shared app state for Tauri.
pub type SharedSessionManager = Arc<Mutex<SessionManager>>;

pub fn new_shared() -> SharedSessionManager {
    Arc::new(Mutex::new(SessionManager::new()))
}

/// Wait until session reaches Selected.
pub fn wait_until_selected(manager: &SharedSessionManager, id: &str, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(g) = manager.lock() {
            if let Some(s) = g.session_ref(id) {
                if s.hsms_state == "Selected" {
                    return true;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{ConnectionMode, Role};
    use crate::session::log::LogDirection;
    use secs4rs::secs2::Secs2;

    fn seed_pair(manager: &SharedSessionManager, port: u16) -> (String, String) {
        let equip = {
            let mut g = manager.lock().unwrap();
            g.create(Some(SessionConfig {
                name: "Equip".into(),
                role: Role::Equipment,
                mode: ConnectionMode::Passive,
                ip: "127.0.0.1".into(),
                port,
                session_id: 10,
                linktest_enabled: false,
                rebind_if_passive: true,
                t3: 5.0,
                t5: 1.0,
                t6: 5.0,
                t7: 10.0,
                t8: 5.0,
                ..SessionConfig::default()
            }))
        };
        let host = {
            let mut g = manager.lock().unwrap();
            g.create(Some(SessionConfig {
                name: "Host".into(),
                role: Role::Host,
                mode: ConnectionMode::Active,
                ip: "127.0.0.1".into(),
                port,
                session_id: 10,
                linktest_enabled: false,
                rebind_if_passive: false,
                t3: 5.0,
                t5: 1.0,
                t6: 5.0,
                t7: 10.0,
                t8: 5.0,
                ..SessionConfig::default()
            }))
        };
        (equip.id, host.id)
    }

    #[test]
    fn restore_snapshot_replaces_sessions() {
        use crate::persistence::{AppPersistState, AppSettings, SessionSnapshot};

        let manager = new_shared();
        {
            let mut g = manager.lock().unwrap();
            g.create(Some(SessionConfig {
                name: "Temp".into(),
                ..SessionConfig::default()
            }));
            assert_eq!(g.list().len(), 1);
            g.restore_snapshot(AppPersistState {
                version: 1,
                settings: AppSettings::default(),
                sessions: vec![
                    SessionSnapshot {
                        id: "e1".into(),
                        config: SessionConfig {
                            name: "Equip".into(),
                            role: Role::Equipment,
                            mode: ConnectionMode::Passive,
                            ..SessionConfig::default()
                        },
                        catalog: MessageCatalog::default(),
                    },
                    SessionSnapshot {
                        id: "h1".into(),
                        config: SessionConfig {
                            name: "Host".into(),
                            role: Role::Host,
                            mode: ConnectionMode::Active,
                            ..SessionConfig::default()
                        },
                        catalog: MessageCatalog::default(),
                    },
                ],
            })
            .unwrap();
            let list = g.list();
            assert_eq!(list.len(), 2);
            assert!(list.iter().any(|s| s.name == "Equip"));
            assert!(list.iter().any(|s| s.name == "Host"));
        }
    }

    #[test]
    fn dual_session_active_passive_select() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let manager = new_shared();
        let (equip_id, host_id) = seed_pair(&manager, port);

        SessionManager::open_session(&manager, &equip_id, None).unwrap();
        thread::sleep(Duration::from_millis(150));
        SessionManager::open_session(&manager, &host_id, None).unwrap();

        assert!(wait_until_selected(&manager, &equip_id, Duration::from_secs(5)));
        assert!(wait_until_selected(&manager, &host_id, Duration::from_secs(5)));

        SessionManager::close_session(&manager, &host_id, None).unwrap();
        SessionManager::close_session(&manager, &equip_id, None).unwrap();
    }

    #[test]
    fn dual_session_s1f1_appears_in_logs() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let manager = new_shared();
        let (equip_id, host_id) = seed_pair(&manager, port);

        SessionManager::open_session(&manager, &equip_id, None).unwrap();
        thread::sleep(Duration::from_millis(150));
        SessionManager::open_session(&manager, &host_id, None).unwrap();
        assert!(wait_until_selected(&manager, &host_id, Duration::from_secs(5)));
        assert!(wait_until_selected(&manager, &equip_id, Duration::from_secs(5)));

        // Host → Equip S1F1 without W-bit (no T3 wait; M5 will auto-reply later).
        SessionManager::send_data(
            &manager,
            &host_id,
            1,
            1,
            false,
            Secs2::list_empty(),
        )
        .unwrap();

        // Allow pass-through / recv path to settle.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut ok = false;
        let mut last_dump = String::new();
        while std::time::Instant::now() < deadline {
            let g = manager.lock().unwrap();
            let host_tx = g.session_ref(&host_id).unwrap().logs.count_data_sf(1, 1);
            let equip_rx = g.session_ref(&equip_id).unwrap().logs.count_data_sf(1, 1);
            last_dump = format!(
                "host={:?} equip={:?}",
                g.session_ref(&host_id)
                    .unwrap()
                    .logs
                    .entries()
                    .iter()
                    .map(|e| format!("{:?}:{}", e.direction, e.summary))
                    .collect::<Vec<_>>(),
                g.session_ref(&equip_id)
                    .unwrap()
                    .logs
                    .entries()
                    .iter()
                    .map(|e| format!("{:?}:{}", e.direction, e.summary))
                    .collect::<Vec<_>>()
            );
            if host_tx >= 1 && equip_rx >= 1 {
                let host_has_tx = g
                    .session_ref(&host_id)
                    .unwrap()
                    .logs
                    .entries()
                    .iter()
                    .any(|e| {
                        e.direction == LogDirection::Tx
                            && e.stream == Some(1)
                            && e.function == Some(1)
                    });
                let equip_has_rx = g
                    .session_ref(&equip_id)
                    .unwrap()
                    .logs
                    .entries()
                    .iter()
                    .any(|e| {
                        e.direction == LogDirection::Rx
                            && e.stream == Some(1)
                            && e.function == Some(1)
                    });
                if host_has_tx && equip_has_rx {
                    ok = true;
                    break;
                }
            }
            drop(g);
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "S1F1 not visible as Host Tx and Equip Rx: {last_dump}");

        SessionManager::close_session(&manager, &host_id, None).unwrap();
        SessionManager::close_session(&manager, &equip_id, None).unwrap();
    }

    #[test]
    fn catalog_auto_reply_s1f13() {
        use crate::catalog::PrefabMessage;
        use uuid::Uuid;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let manager = new_shared();
        let (equip_id, host_id) = seed_pair(&manager, port);

        // Equip catalog: S1F13 AR + S1F14 body
        {
            let mut g = manager.lock().unwrap();
            g.set_catalog(
                &equip_id,
                MessageCatalog {
                    source: "test".into(),
                    messages: vec![
                        PrefabMessage {
                            id: Uuid::new_v4().to_string(),
                            message_name: "ConnectEquip".into(),
                            description: "Establish".into(),
                            pair_name: "S1F13".into(),
                            stream: 1,
                            function: 13,
                            direction: "H->E".into(),
                            wait: true,
                            auto_reply: true,
                            no_logging: false,
                            body_sml: String::new(),
                            body_tree: vec![],
                        },
                        PrefabMessage {
                            id: Uuid::new_v4().to_string(),
                            message_name: "EquipConnected".into(),
                            description: "Establish".into(),
                            pair_name: "S1F13".into(),
                            stream: 1,
                            function: 14,
                            direction: "H<-E".into(),
                            wait: false,
                            auto_reply: false,
                            no_logging: false,
                            body_sml: r#"<L <B 0x00> <L <A "SECS-SIM"> <A "0.1.0"> >>"#.into(),
                            body_tree: vec![],
                        },
                    ],
                },
            )
            .unwrap();
        }

        SessionManager::open_session(&manager, &equip_id, None).unwrap();
        thread::sleep(Duration::from_millis(150));
        SessionManager::open_session(&manager, &host_id, None).unwrap();
        assert!(wait_until_selected(&manager, &host_id, Duration::from_secs(5)));

        let res = SessionManager::send_sml(&manager, &host_id, "S1F13 W.").unwrap();
        let reply = res.reply.expect("S1F14");
        assert_eq!((reply.stream, reply.function), (1, 14));

        SessionManager::close_session(&manager, &host_id, None).unwrap();
        SessionManager::close_session(&manager, &equip_id, None).unwrap();
    }

    #[test]
    fn send_sml_primary_and_logs() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let manager = new_shared();
        let (equip_id, host_id) = seed_pair(&manager, port);

        SessionManager::open_session(&manager, &equip_id, None).unwrap();
        thread::sleep(Duration::from_millis(150));
        SessionManager::open_session(&manager, &host_id, None).unwrap();
        assert!(wait_until_selected(&manager, &host_id, Duration::from_secs(5)));

        // Invalid SML rejected with clear error.
        let err = SessionManager::send_sml(&manager, &host_id, "S1F1 W").unwrap_err();
        assert!(err.to_string().contains("SML"), "{err}");

        // Full SML primary (no W — peer has no auto-reply yet).
        let res = SessionManager::send_sml(
            &manager,
            &host_id,
            r#"S2F17 <A "20260806120000">."#,
        )
        .unwrap();
        assert_eq!(res.stream, 2);
        assert_eq!(res.function, 17);
        assert!(!res.wbit);
        assert!(res.reply.is_none());

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut ok = false;
        while std::time::Instant::now() < deadline {
            let g = manager.lock().unwrap();
            let host_tx = g.session_ref(&host_id).unwrap().logs.count_data_sf(2, 17);
            let equip_rx = g.session_ref(&equip_id).unwrap().logs.count_data_sf(2, 17);
            if host_tx >= 1 && equip_rx >= 1 {
                ok = true;
                break;
            }
            drop(g);
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "S2F17 SML send not visible in Tx/Rx logs");

        SessionManager::close_session(&manager, &host_id, None).unwrap();
        SessionManager::close_session(&manager, &equip_id, None).unwrap();
    }

    /// Host sends S6F11 W (no Equip AutoReply). Equip manually sends S6F12
    /// which must reuse the primary system-bytes so Host's T3 waiter completes.
    #[test]
    fn host_s6f11_manual_s6f12_reply() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../doc1.SMD");
        if !path.is_file() {
            return;
        }
        let xml = std::fs::read_to_string(&path).unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let manager = new_shared();
        let (equip_id, host_id) = seed_pair(&manager, port);
        {
            let mut g = manager.lock().unwrap();
            g.import_smd(&equip_id, &xml, "doc1.SMD").unwrap();
            g.import_smd(&host_id, &xml, "doc1.SMD").unwrap();
        }

        let (s6f11_body, s6f12_body) = {
            let g = manager.lock().unwrap();
            let cat = g.get_catalog(&host_id).unwrap();
            let p = cat
                .messages
                .iter()
                .find(|m| m.stream == 6 && m.function == 11)
                .expect("doc1 S6F11");
            let r = cat
                .messages
                .iter()
                .find(|m| m.stream == 6 && m.function == 12)
                .expect("doc1 S6F12");
            (p.body_secs2().unwrap(), r.body_secs2().unwrap())
        };

        SessionManager::open_session(&manager, &equip_id, None).unwrap();
        thread::sleep(Duration::from_millis(150));
        SessionManager::open_session(&manager, &host_id, None).unwrap();
        assert!(wait_until_selected(&manager, &host_id, Duration::from_secs(5)));

        let mgr = Arc::clone(&manager);
        let hid = host_id.clone();
        let waiter = thread::spawn(move || {
            SessionManager::send_data(&mgr, &hid, 6, 11, true, s6f11_body)
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut saw_rx = false;
        while std::time::Instant::now() < deadline {
            let g = manager.lock().unwrap();
            if g.session_ref(&equip_id)
                .unwrap()
                .logs
                .count_data_sf(6, 11)
                >= 1
            {
                saw_rx = true;
                break;
            }
            drop(g);
            thread::sleep(Duration::from_millis(30));
        }
        assert!(saw_rx, "Equip did not receive S6F11");

        SessionManager::send_data(&manager, &equip_id, 6, 12, false, s6f12_body)
            .expect("manual S6F12");

        let reply = waiter
            .join()
            .expect("send thread")
            .expect("Host T3")
            .expect("S6F12 reply");
        assert_eq!((reply.get_stream(), reply.get_function()), (6, 12));

        SessionManager::close_session(&manager, &host_id, None).unwrap();
        SessionManager::close_session(&manager, &equip_id, None).unwrap();
    }
}
