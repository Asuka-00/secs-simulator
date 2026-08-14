//! Rule table CRUD commands.

use crate::error::AppResult;
use crate::rules::RuleSet;
use crate::session::SharedSessionManager;

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

#[tauri::command]
pub fn session_get_rules(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
) -> AppResult<RuleSet> {
    let guard = manager.lock().map_err(lock_err)?;
    guard.get_rules(&id)
}

#[tauri::command]
pub fn session_set_rules(
    manager: tauri::State<'_, SharedSessionManager>,
    id: String,
    rules: RuleSet,
) -> AppResult<()> {
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.set_rules(&id, rules)
}
