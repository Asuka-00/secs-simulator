//! Per-session flow CRUD + run/stop.

use crate::error::AppResult;
use crate::flow::Flow;
use crate::session::{SessionManager, SharedSessionManager};

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

#[tauri::command]
pub fn session_get_flows(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<Vec<Flow>> {
    let guard = manager.lock().map_err(lock_err)?;
    guard.get_flows(&id)
}

#[tauri::command]
pub fn session_set_flows(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    flows: Vec<Flow>,
) -> AppResult<()> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.set_flows(&id, flows)
}

#[tauri::command]
pub fn flow_run(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    flow_id: String,
) -> AppResult<()> {
    SessionManager::flow_run(manager.inner(), &id, &flow_id)
}

#[tauri::command]
pub fn flow_stop(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    flow_id: String,
) -> AppResult<()> {
    SessionManager::flow_stop(manager.inner(), &id, &flow_id)
}

#[tauri::command]
pub fn session_running_flows(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<Vec<String>> {
    Ok(SessionManager::running_flow_ids(manager.inner(), &id))
}
