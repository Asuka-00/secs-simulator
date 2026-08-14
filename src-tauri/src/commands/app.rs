//! App-level commands (settings, version, persist).

use tauri::AppHandle;

use crate::error::AppResult;
use crate::persistence::{
    load_app_state, save_app_state, AppPersistState, AppSettings,
};
use crate::session::SharedSessionManager;

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

/// Smoke command: prove vendored `secs4rs` package links correctly.
#[tauri::command]
pub fn secs4rs_version() -> String {
    secs4rs::VERSION.to_string()
}

#[tauri::command]
pub fn app_get_settings(
    manager: tauri::State<'_, SharedSessionManager>,
) -> AppResult<AppSettings> {
    let guard = manager.lock().map_err(lock_err)?;
    Ok(guard.settings().clone())
}

#[tauri::command]
pub fn app_set_settings(
    app: AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
    settings: AppSettings,
) -> AppResult<()> {
    {
        let mut guard = manager.lock().map_err(lock_err)?;
        guard.set_settings(settings);
    }
    // Persist immediately.
    let state = {
        let guard = manager.lock().map_err(lock_err)?;
        guard.snapshot()
    };
    save_app_state(&app, &state)?;
    Ok(())
}

/// Save full app state (settings + sessions) to app_data_dir.
#[tauri::command]
pub fn app_save_state(
    app: AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
) -> AppResult<()> {
    let state = {
        let guard = manager.lock().map_err(lock_err)?;
        guard.snapshot()
    };
    save_app_state(&app, &state)
}

/// Load app state from disk into memory (requires no open sessions).
#[tauri::command]
pub fn app_load_state(
    app: AppHandle,
    manager: tauri::State<'_, SharedSessionManager>,
) -> AppResult<AppPersistState> {
    let state = load_app_state(&app)?;
    {
        let mut guard = manager.lock().map_err(lock_err)?;
        if state.settings.restore_sessions {
            guard.restore_snapshot(state.clone())?;
        } else {
            guard.set_settings(state.settings.clone());
        }
    }
    Ok(state)
}

/// Return current in-memory snapshot (debug / UI).
#[tauri::command]
pub fn app_get_state(
    manager: tauri::State<'_, SharedSessionManager>,
) -> AppResult<AppPersistState> {
    let guard = manager.lock().map_err(lock_err)?;
    Ok(guard.snapshot())
}
