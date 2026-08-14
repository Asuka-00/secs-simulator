//! SECS Simulator — Tauri backend.

mod catalog;
mod commands;
mod error;
mod persistence;
mod session;
mod sml_bridge;

// Keep legacy modules available for gradual removal (not registered as commands).
#[allow(dead_code)]
mod gem_bridge;
#[allow(dead_code)]
mod rules;

use commands::app::{
    app_get_settings, app_get_state, app_load_state, app_save_state, app_set_settings,
    secs4rs_version,
};
use commands::catalog::{
    session_get_catalog, session_import_smd, session_new_blank_message, session_remove_message,
    session_send_message, session_set_catalog, session_upsert_message,
};
use commands::message::{
    session_clear_logs, session_export_logs, session_get_logs, session_send_sml, sml_parse,
};
use commands::scenario::{
    scenario_export, scenario_export_path, scenario_get, scenario_import, scenario_import_path,
};
use commands::session::{
    session_close, session_create, session_create_from_template, session_create_loopback_pair,
    session_get_config, session_list, session_list_templates, session_open, session_remove,
    session_update_config,
};
use session::new_shared;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(new_shared())
        .setup(|app| {
            let manager = app.state::<session::SharedSessionManager>();
            match persistence::load_app_state(app.handle()) {
                Ok(state) if state.settings.restore_sessions && !state.sessions.is_empty() => {
                    if let Ok(mut g) = manager.lock() {
                        let _ = g.restore_snapshot(state);
                    }
                }
                Ok(state) => {
                    if let Ok(mut g) = manager.lock() {
                        g.set_settings(state.settings);
                    }
                }
                Err(_) => {}
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            secs4rs_version,
            app_get_settings,
            app_set_settings,
            app_save_state,
            app_load_state,
            app_get_state,
            session_list,
            session_create,
            session_remove,
            session_update_config,
            session_get_config,
            session_open,
            session_close,
            session_get_logs,
            session_clear_logs,
            session_export_logs,
            session_list_templates,
            session_create_from_template,
            session_create_loopback_pair,
            session_get_catalog,
            session_set_catalog,
            session_upsert_message,
            session_remove_message,
            session_import_smd,
            session_new_blank_message,
            session_send_message,
            scenario_export,
            scenario_import,
            scenario_export_path,
            scenario_import_path,
            scenario_get,
            sml_parse,
            session_send_sml,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
