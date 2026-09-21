//! Live per-session flow scheduler (interval / inbound / Selected / manual).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use secs4rs::hsms::HsmsMessage;
use tauri::AppHandle;

use crate::catalog::MessageCatalog;
use crate::error::{AppError, AppResult};
use crate::session::config::Role;
use crate::session::log::LogEntry;
use crate::session::runtime::{emit_session_event, push_log, SessionEvent};
use crate::session::{SessionManager, SharedSessionManager};

use super::eval::{compact_sml, NodeResult};
use super::model::{Flow, SendData, WaitData};
use super::walk::walk;

struct RunHandle {
    stop: Arc<AtomicBool>,
    inbound_tx: Option<SyncSender<HsmsMessage>>,
}

#[derive(Clone)]
pub struct FlowRuntime {
    session_id: String,
    manager: SharedSessionManager,
    app: Option<AppHandle>,
    stop: Arc<AtomicBool>,
    flows: Arc<Mutex<Vec<Flow>>>,
    running: Arc<Mutex<HashMap<String, RunHandle>>>,
    last_hsms: Arc<Mutex<String>>,
}

impl FlowRuntime {
    pub fn new(
        session_id: impl Into<String>,
        flows: Vec<Flow>,
        manager: SharedSessionManager,
        app: Option<AppHandle>,
    ) -> Self {
        let rt = Self {
            session_id: session_id.into(),
            manager,
            app,
            stop: Arc::new(AtomicBool::new(false)),
            flows: Arc::new(Mutex::new(flows)),
            running: Arc::new(Mutex::new(HashMap::new())),
            last_hsms: Arc::new(Mutex::new(String::new())),
        };
        rt.spawn_interval();
        rt
    }

    pub fn set_flows(&self, flows: Vec<Flow>) {
        if let Ok(mut g) = self.flows.lock() {
            *g = flows;
        }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.running.lock() {
            for h in g.values() {
                h.stop.store(true, Ordering::SeqCst);
            }
            g.clear();
        }
    }

    pub fn is_running(&self, flow_id: &str) -> bool {
        self.running
            .lock()
            .map(|g| g.contains_key(flow_id))
            .unwrap_or(false)
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.running
            .lock()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn stop_flow(&self, flow_id: &str) {
        if let Ok(mut g) = self.running.lock() {
            if let Some(h) = g.get_mut(flow_id) {
                h.stop.store(true, Ordering::SeqCst);
                h.inbound_tx.take();
            }
        }
    }

    pub fn on_hsms(&self, label: &str) {
        let prev = {
            let Ok(mut g) = self.last_hsms.lock() else {
                return;
            };
            let prev = g.clone();
            *g = label.to_string();
            prev
        };
        if label == "Selected" && prev != "Selected" {
            let flows = self.flows_snapshot();
            for f in flows {
                if f.enabled && f.trigger_data().is_some_and(|t| t.kind == "onSelected") {
                    let _ = self.start_inner(&f.id, None, false);
                }
            }
        }
    }

    pub fn on_inbound(&self, msg: &HsmsMessage) {
        if let Ok(g) = self.running.lock() {
            for h in g.values() {
                let Some(tx) = h.inbound_tx.as_ref() else {
                    continue;
                };
                match tx.try_send(msg.clone()) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => {}
                }
            }
        }
        if !msg.is_data_message() {
            return;
        }
        let stream = msg.get_stream();
        let function = msg.get_function();
        let start = Some(NodeResult::from_hsms(msg));
        for f in self.flows_snapshot() {
            if f.enabled && f.matches_inbound(stream, function) {
                let _ = self.start_inner(&f.id, start.clone(), false);
            }
        }
    }

    pub fn start_manual(&self, flow_id: &str) -> AppResult<()> {
        self.start_inner(flow_id, None, true)
    }

    fn flows_snapshot(&self) -> Vec<Flow> {
        self.flows.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn start_inner(
        &self,
        flow_id: &str,
        start: Option<NodeResult>,
        fail_if_running: bool,
    ) -> AppResult<()> {
        if self.stop.load(Ordering::SeqCst) {
            return Err(AppError::Message("session closed".into()));
        }
        let flow = self
            .flows_snapshot()
            .into_iter()
            .find(|f| f.id == flow_id)
            .ok_or_else(|| AppError::Message(format!("flow not found: {flow_id}")))?;

        let (tx, rx) = sync_channel::<HsmsMessage>(32);
        let run_stop = Arc::new(AtomicBool::new(false));
        {
            let mut g = self
                .running
                .lock()
                .map_err(|e| AppError::Message(e.to_string()))?;
            if g.contains_key(flow_id) {
                if fail_if_running {
                    return Err(AppError::Message("flow already running".into()));
                }
                return Ok(());
            }
            g.insert(
                flow_id.to_string(),
                RunHandle {
                    stop: Arc::clone(&run_stop),
                    inbound_tx: Some(tx),
                },
            );
        }

        let sid = self.session_id.clone();
        let mgr = Arc::clone(&self.manager);
        let app = self.app.clone();
        let global_stop = Arc::clone(&self.stop);
        let running = Arc::clone(&self.running);
        let fid = flow.id.clone();
        let fname = flow.name.clone();

        push_log(
            &mgr,
            &app,
            &sid,
            LogEntry::system(format!("flow `{fname}` start")),
        );

        thread::spawn(move || {
            let stopped = {
                let gs = Arc::clone(&global_stop);
                let rs = Arc::clone(&run_stop);
                move || gs.load(Ordering::SeqCst) || rs.load(Ordering::SeqCst)
            };
            let result = walk(
                &flow,
                start,
                {
                    let mgr = Arc::clone(&mgr);
                    let sid = sid.clone();
                    let gs = Arc::clone(&global_stop);
                    let rs = Arc::clone(&run_stop);
                    move |data| send_until_stop(&mgr, &sid, data, &gs, &rs)
                },
                {
                    let st = Arc::clone(&global_stop);
                    let rs = Arc::clone(&run_stop);
                    move |wd| wait_inbound(&rx, wd, &st, &rs)
                },
                |ms| delay_ms(ms, &global_stop, &run_stop),
                stopped,
                {
                    let app = app.clone();
                    let sid = sid.clone();
                    let fid = fid.clone();
                    move |node_id, status| {
                        emit_session_event(
                            &app,
                            SessionEvent::flow_progress(&sid, &fid, node_id, status),
                        );
                    }
                },
            );

            if let Ok(mut g) = running.lock() {
                g.remove(&fid);
            }

            match result {
                Ok(()) => {
                    push_log(
                        &mgr,
                        &app,
                        &sid,
                        LogEntry::system(format!("flow `{fname}` done")),
                    );
                    emit_session_event(&app, SessionEvent::flow_done(&sid, &fid, "ok"));
                }
                Err(e) => {
                    let msg = e.to_string();
                    push_log(
                        &mgr,
                        &app,
                        &sid,
                        LogEntry::system(format!("flow `{fname}` {msg}")),
                    );
                    emit_session_event(&app, SessionEvent::flow_done(&sid, &fid, &msg));
                }
            }
        });
        Ok(())
    }

    fn spawn_interval(&self) {
        let stop = Arc::clone(&self.stop);
        let flows = Arc::clone(&self.flows);
        let rt = self.clone();
        thread::spawn(move || {
            let mut last: HashMap<String, Instant> = HashMap::new();
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(200));
                let list = flows.lock().map(|g| g.clone()).unwrap_or_default();
                for f in list {
                    if !f.enabled {
                        continue;
                    }
                    let Some(t) = f.trigger_data() else {
                        continue;
                    };
                    if t.kind != "interval" {
                        continue;
                    }
                    let iv = t.interval_ms.unwrap_or(5_000).max(200);
                    match last.get(&f.id) {
                        None => {
                            last.insert(f.id.clone(), Instant::now());
                        }
                        Some(at) if at.elapsed().as_millis() as u64 >= iv => {
                            last.insert(f.id.clone(), Instant::now());
                            let _ = rt.start_inner(&f.id, None, false);
                        }
                        Some(_) => {}
                    }
                }
            }
        });
    }
}

fn delay_ms(ms: u64, global: &AtomicBool, run: &AtomicBool) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        if global.load(Ordering::SeqCst) || run.load(Ordering::SeqCst) {
            return;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        thread::sleep(left.min(Duration::from_millis(50)));
    }
}

fn wait_inbound(
    rx: &Receiver<HsmsMessage>,
    wd: &WaitData,
    global: &AtomicBool,
    run: &AtomicBool,
) -> AppResult<NodeResult> {
    let deadline = Instant::now() + Duration::from_millis(wd.timeout_ms.max(1));
    loop {
        if global.load(Ordering::SeqCst) || run.load(Ordering::SeqCst) {
            return Err(AppError::Message("flow stopped".into()));
        }
        let rem = deadline.saturating_duration_since(Instant::now());
        if rem.is_zero() {
            return Err(AppError::Message(format!(
                "wait S{}F{} timeout",
                wd.stream, wd.function
            )));
        }
        match rx.recv_timeout(rem.min(Duration::from_millis(200))) {
            Ok(msg)
                if msg.is_data_message()
                    && msg.get_stream() == wd.stream
                    && msg.get_function() == wd.function =>
            {
                return Ok(NodeResult::from_hsms(&msg));
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AppError::Message("flow stopped".into()));
            }
        }
    }
}

fn dir_norm(s: &str) -> String {
    s.replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace(' ', "")
}

fn resolve_send(cat: &MessageCatalog, role: &Role, data: &SendData) -> AppResult<crate::catalog::PrefabMessage> {
    if let Some(id) = data.message_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(m) = cat.get(id) {
            return Ok(m.clone());
        }
    }
    let stream = data
        .stream
        .ok_or_else(|| AppError::Message("send node missing stream".into()))?;
    let function = data
        .function
        .ok_or_else(|| AppError::Message("send node missing function".into()))?;
    let dir = data.direction.as_deref().unwrap_or("");
    let found = cat
        .messages
        .iter()
        .find(|m| {
            m.stream == stream
                && m.function == function
                && !dir.is_empty()
                && dir_norm(&m.direction) == dir_norm(dir)
        })
        .or_else(|| {
            cat.messages
                .iter()
                .find(|m| m.stream == stream && m.function == function && m.is_outbound_for(role))
        })
        .or_else(|| {
            cat.messages
                .iter()
                .find(|m| m.stream == stream && m.function == function)
        });
    found
        .cloned()
        .ok_or_else(|| AppError::Message(format!("catalog has no S{stream}F{function}")))
}

fn send_until_stop(
    manager: &SharedSessionManager,
    session_id: &str,
    data: &SendData,
    global: &AtomicBool,
    run: &AtomicBool,
) -> AppResult<Option<NodeResult>> {
    let (tx, rx) = sync_channel(1);
    let mgr = Arc::clone(manager);
    let sid = session_id.to_string();
    let data = data.clone();
    thread::spawn(move || {
        let _ = tx.send(resolve_and_send(&mgr, &sid, &data));
    });
    loop {
        if global.load(Ordering::SeqCst) || run.load(Ordering::SeqCst) {
            return Err(AppError::Message("flow stopped".into()));
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(r) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AppError::Message("flow stopped".into()));
            }
        }
    }
}

fn resolve_and_send(
    manager: &SharedSessionManager,
    session_id: &str,
    data: &SendData,
) -> AppResult<Option<NodeResult>> {
    let (role, cat) = {
        let g = manager
            .lock()
            .map_err(|e| AppError::Message(e.to_string()))?;
        let s = g
            .session_ref(session_id)
            .ok_or_else(|| AppError::Message("session not found".into()))?;
        (s.config.role.clone(), s.catalog.clone())
    };
    let msg = resolve_send(&cat, &role, data)?;
    let body = msg.body_secs2()?;
    let stream = msg.stream;
    let function = msg.function;
    let wbit = msg.wait;
    let sent_body = body.clone();
    let reply = SessionManager::send_data(manager, session_id, stream, function, wbit, body)?;
    Ok(Some(match reply {
        Some(r) => NodeResult::from_hsms(&r),
        None => NodeResult {
            stream,
            function,
            sml: compact_sml(stream, function, &sent_body),
            body: sent_body,
        },
    }))
}
