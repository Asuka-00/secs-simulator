//! Scenario import/export commands.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::error::AppResult;
use crate::persistence::{
    read_scenario_file, scenario_from_json, scenario_to_json, write_scenario_file, ScenarioFile,
};
use crate::session::SharedSessionManager;

fn lock_err(e: impl ToString) -> crate::error::AppError {
    crate::error::AppError::Message(e.to_string())
}

/// Export current sessions as scenario JSON string.
#[tauri::command]
pub fn scenario_export(
    manager: tauri::State<'_, SharedSessionManager>,
    name: Option<String>,
) -> AppResult<String> {
    let guard = manager.lock().map_err(lock_err)?;
    let sc = guard.to_scenario(name.unwrap_or_else(|| "secs-scenario".into()));
    scenario_to_json(&sc)
}

/// Import scenario from JSON string (replaces all sessions; none may be open).
#[tauri::command]
pub fn scenario_import(
    manager: tauri::State<'_, SharedSessionManager>,
    json: String,
) -> AppResult<usize> {
    let sc = scenario_from_json(&json)?;
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.import_scenario(sc)
}

/// Write scenario JSON to a filesystem path.
#[tauri::command]
pub fn scenario_export_path(
    manager: tauri::State<'_, SharedSessionManager>,
    path: String,
    name: Option<String>,
) -> AppResult<()> {
    let guard = manager.lock().map_err(lock_err)?;
    let sc = guard.to_scenario(name.unwrap_or_else(|| "secs-scenario".into()));
    write_scenario_file(PathBuf::from(path).as_path(), &sc)
}

/// Read scenario file from path and import.
#[tauri::command]
pub fn scenario_import_path(
    manager: tauri::State<'_, SharedSessionManager>,
    path: String,
) -> AppResult<usize> {
    let sc = read_scenario_file(PathBuf::from(path).as_path())?;
    let mut guard = manager.lock().map_err(lock_err)?;
    guard.import_scenario(sc)
}

/// Convenience: export + save app state after import workflows from UI.
#[tauri::command]
pub fn scenario_list_names(_app: AppHandle) -> AppResult<Vec<String>> {
    // Reserved for future multi-scenario library in app_data.
    Ok(Vec::new())
}

/// Build scenario DTO without serializing (for typed frontend if needed).
#[tauri::command]
pub fn scenario_get(
    manager: tauri::State<'_, SharedSessionManager>,
    name: Option<String>,
) -> AppResult<ScenarioFile> {
    let guard = manager.lock().map_err(lock_err)?;
    Ok(guard.to_scenario(name.unwrap_or_else(|| "secs-scenario".into())))
}
