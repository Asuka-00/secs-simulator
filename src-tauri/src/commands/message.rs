//! Message log + SML send commands.

use crate::error::{AppError, AppResult};
use crate::session::log::{export_logs, LogEntry, LogExportFormat};
use crate::session::{SessionManager, SharedSessionManager};
use crate::sml_bridge::{parsed_preview, ParsedSml, SendSmlResult};

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

#[tauri::command]
pub fn session_get_logs(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<Vec<LogEntry>> {
    let guard = manager.lock().map_err(lock_err)?;
    guard.get_logs(&id)
}

#[tauri::command]
pub fn session_clear_logs(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<()> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.clear_logs(&id)
}

/// Export session logs as JSON or text (filtered client-side may pass entries later).
#[tauri::command]
pub fn session_export_logs(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    format: LogExportFormat,
) -> AppResult<String> {
    let guard = manager.lock().map_err(lock_err)?;
    let entries = guard.get_logs(&id)?;
    export_logs(&entries, format).map_err(AppError::Message)
}

/// Validate SML without sending.
#[tauri::command]
pub fn sml_parse(text: String) -> AppResult<ParsedSml> {
    parsed_preview(&text)
}

/// Parse SML and send as primary DATA. W-bit primaries wait T3 in the background.
#[tauri::command]
pub fn session_send_sml(
    app: tauri::AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    sml: String,
) -> AppResult<SendSmlResult> {
    use crate::sml_bridge::{parse_sml, reply_info, SendSmlResult};

    let msg = parse_sml(&sml)?;
    let stream = msg.get_stream();
    let function = msg.get_function();
    let wbit = msg.wbit();
    let body = msg.secs2().clone();
    let w = if wbit { " W" } else { "" };
    let summary = format!("S{stream}F{function}{w}");
    let waiting = wbit && function % 2 == 1;
    let reply_msg = SessionManager::send_data_ui(
        manager.inner(),
        &id,
        stream,
        function,
        wbit,
        body,
        Some(app),
    )?;
    Ok(SendSmlResult {
        stream,
        function,
        wbit,
        summary,
        reply: reply_msg.as_ref().map(reply_info),
        waiting,
    })
}
