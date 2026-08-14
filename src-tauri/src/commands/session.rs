//! Session CRUD + open/close + templates commands.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::session::config::SessionConfig;
use crate::session::templates::{builtin_templates, loopback_pair_templates, SessionTemplate};
use crate::session::{SessionManager, SessionSummary, SharedSessionManager};

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

#[tauri::command]
pub fn session_list(
    manager: tauri::State<'_, SharedSessionManager>,
) -> AppResult<Vec<SessionSummary>> {
    let guard = manager.lock().map_err(lock_err)?;
    Ok(guard.list())
}

#[tauri::command]
pub fn session_create(
    manager: tauri::State<'_, SharedSessionManager>,
    config: Option<SessionConfig>,
) -> AppResult<SessionSummary> {
    let mut guard = manager.lock().map_err(lock_err)?;
    Ok(guard.create(config))
}

#[tauri::command]
pub fn session_remove(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<()> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.remove(&id)
}

#[tauri::command]
pub fn session_update_config(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    config: SessionConfig,
) -> AppResult<SessionSummary> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.update_config(&id, config)
}

#[tauri::command]
pub fn session_get_config(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<SessionConfig> {
    let guard = manager.lock().map_err(lock_err)?;
    guard.get_config(&id)
}

#[tauri::command]
pub fn session_open(
    app: AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<SessionSummary> {
    SessionManager::open_session(manager.inner(), &id, Some(app))
}

#[tauri::command]
pub fn session_close(
    app: AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<SessionSummary> {
    SessionManager::close_session(manager.inner(), &id, Some(app))
}

/// List built-in session config templates.
#[tauri::command]
pub fn session_list_templates() -> Vec<SessionTemplate> {
    builtin_templates()
}

/// Create a session from a built-in template id.
#[tauri::command]
pub fn session_create_from_template(
    manager: tauri::State<'_, SharedSessionManager>,
    template_id: String,
) -> AppResult<SessionSummary> {
    let tmpl = builtin_templates()
        .into_iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| crate::error::AppError::Message(format!("unknown template: {template_id}")))?;
    let mut guard = manager.lock().map_err(lock_err)?;
    Ok(guard.create(Some(tmpl.config)))
}

/// Create Equip+Host loopback pair on a port (returns both summaries).
#[tauri::command]
pub fn session_create_loopback_pair(
    manager: tauri::State<'_, SharedSessionManager>,
    port: Option<u16>,
) -> AppResult<Vec<SessionSummary>> {
    let port = port.unwrap_or(5000);
    let mut guard = manager.lock().map_err(lock_err)?;
    let mut out = Vec::new();
    for cfg in loopback_pair_templates(port) {
        out.push(guard.create(Some(cfg)));
    }
    Ok(out)
}
