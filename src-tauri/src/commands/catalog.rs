//! Message catalog commands (SMD import, CRUD, send prefab).

use crate::catalog::{new_blank_message, MessageCatalog, PrefabMessage};
use crate::error::AppResult;
use crate::session::{SessionManager, SharedSessionManager};
use crate::sml_bridge::{reply_info, SendSmlResult};

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

#[tauri::command]
pub fn session_get_catalog(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<MessageCatalog> {
    let guard = manager.lock().map_err(lock_err)?;
    guard.get_catalog(&id)
}

#[tauri::command]
pub fn session_set_catalog(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    catalog: MessageCatalog,
) -> AppResult<()> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.set_catalog(&id, catalog)
}

#[tauri::command]
pub fn session_upsert_message(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    message: PrefabMessage,
) -> AppResult<MessageCatalog> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.upsert_message(&id, message)
}

#[tauri::command]
pub fn session_remove_message(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    message_id: String,
) -> AppResult<MessageCatalog> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.remove_message(&id, &message_id)
}

#[tauri::command]
pub fn session_import_smd(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    xml: String,
    source: Option<String>,
) -> AppResult<MessageCatalog> {
    let mut guard = manager.lock().map_err(lock_err)?;
    let src = source.unwrap_or_else(|| "import.smd".into());
    let (_n, cat) = guard.import_smd(&id, &xml, &src)?;
    Ok(cat)
}

#[tauri::command]
pub fn session_new_blank_message() -> PrefabMessage {
    new_blank_message()
}

/// Send a prefabricated message (uses Wait as W-bit).
/// W-bit primaries return immediately; T3 wait runs in the background.
#[tauri::command]
pub fn session_send_message(
    app: tauri::AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    message: PrefabMessage,
) -> AppResult<SendSmlResult> {
    let body = message.body_secs2()?;
    let stream = message.stream;
    let function = message.function;
    let wbit = message.wait;
    let w = if wbit { " W" } else { "" };
    let summary = format!(
        "S{stream}F{function}{w} ({})",
        message.message_name
    );

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
    let reply = reply_msg.as_ref().map(reply_info);

    Ok(SendSmlResult {
        stream,
        function,
        wbit,
        summary,
        reply,
        waiting,
    })
}
