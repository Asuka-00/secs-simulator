//! App data dir persistence & scenario import/export.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::catalog::MessageCatalog;
use crate::error::{AppError, AppResult};
use crate::session::config::SessionConfig;

pub const STATE_FILE: &str = "app-state.json";
pub const SCENARIO_EXT: &str = "secs-scenario.json";

/// Global app preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_log_capacity")]
    pub log_capacity: usize,
    /// Restore last session list on startup.
    #[serde(default = "default_true")]
    pub restore_sessions: bool,
}

fn default_theme() -> String {
    "dark".into()
}
fn default_log_capacity() -> usize {
    5000
}
fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            log_capacity: default_log_capacity(),
            restore_sessions: true,
        }
    }
}

/// One session snapshot (no logs / no open runtime).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: String,
    pub config: SessionConfig,
    #[serde(default)]
    pub catalog: MessageCatalog,
}

/// Full app state file in app_data_dir.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppPersistState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub sessions: Vec<SessionSnapshot>,
}

fn default_version() -> u32 {
    1
}

impl Default for AppPersistState {
    fn default() -> Self {
        Self {
            version: 1,
            settings: AppSettings::default(),
            sessions: Vec::new(),
        }
    }
}

/// Portable scenario file (`.secs-scenario.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sessions: Vec<SessionSnapshot>,
}

impl Default for ScenarioFile {
    fn default() -> Self {
        Self {
            version: 1,
            name: "scenario".into(),
            sessions: Vec::new(),
        }
    }
}

pub fn app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Message(format!("app_data_dir: {e}")))
}

pub fn state_file_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join(STATE_FILE))
}

pub fn ensure_app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app_data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| AppError::Message(format!("create app_data: {e}")))?;
    Ok(dir)
}

pub fn save_state_to(path: &Path, state: &AppPersistState) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Message(format!("create parent: {e}")))?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| AppError::Message(format!("serialize state: {e}")))?;
    fs::write(path, json).map_err(|e| AppError::Message(format!("write state: {e}")))?;
    Ok(())
}

pub fn load_state_from(path: &Path) -> AppResult<AppPersistState> {
    if !path.exists() {
        return Ok(AppPersistState::default());
    }
    let raw =
        fs::read_to_string(path).map_err(|e| AppError::Message(format!("read state: {e}")))?;
    serde_json::from_str(&raw).map_err(|e| AppError::Message(format!("parse state: {e}")))
}

pub fn save_app_state(app: &AppHandle, state: &AppPersistState) -> AppResult<()> {
    ensure_app_data_dir(app)?;
    save_state_to(&state_file_path(app)?, state)
}

pub fn load_app_state(app: &AppHandle) -> AppResult<AppPersistState> {
    let path = state_file_path(app)?;
    load_state_from(&path)
}

pub fn scenario_to_json(scenario: &ScenarioFile) -> AppResult<String> {
    serde_json::to_string_pretty(scenario)
        .map_err(|e| AppError::Message(format!("serialize scenario: {e}")))
}

pub fn scenario_from_json(json: &str) -> AppResult<ScenarioFile> {
    serde_json::from_str(json.trim())
        .map_err(|e| AppError::Message(format!("parse scenario: {e}")))
}

pub fn write_scenario_file(path: &Path, scenario: &ScenarioFile) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Message(format!("create parent: {e}")))?;
    }
    let json = scenario_to_json(scenario)?;
    fs::write(path, json).map_err(|e| AppError::Message(format!("write scenario: {e}")))?;
    Ok(())
}

pub fn read_scenario_file(path: &Path) -> AppResult<ScenarioFile> {
    let raw = fs::read_to_string(path)
        .map_err(|e| AppError::Message(format!("read scenario: {e}")))?;
    scenario_from_json(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{ConnectionMode, Role, SessionConfig};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_path(name: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("secs-sim-{n}-{name}"))
    }

    #[test]
    fn roundtrip_app_state_file() {
        let path = tmp_path("state.json");
        let state = AppPersistState {
            version: 1,
            settings: AppSettings {
                theme: "dark".into(),
                log_capacity: 1000,
                restore_sessions: true,
            },
            sessions: vec![SessionSnapshot {
                id: "aaa".into(),
                config: SessionConfig {
                    name: "Equip".into(),
                    role: Role::Equipment,
                    mode: ConnectionMode::Passive,
                    port: 5000,
                    ..SessionConfig::default()
                },
                catalog: MessageCatalog::default(),
            }],
        };
        save_state_to(&path, &state).unwrap();
        let loaded = load_state_from(&path).unwrap();
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].config.name, "Equip");
        assert_eq!(loaded.settings.log_capacity, 1000);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn scenario_json_roundtrip() {
        let sc = ScenarioFile {
            version: 1,
            name: "loopback".into(),
            sessions: vec![SessionSnapshot {
                id: "h1".into(),
                config: SessionConfig {
                    name: "Host".into(),
                    role: Role::Host,
                    mode: ConnectionMode::Active,
                    ..SessionConfig::default()
                },
                catalog: MessageCatalog::default(),
            }],
        };
        let json = scenario_to_json(&sc).unwrap();
        assert!(json.contains("secs") || json.contains("Host") || json.contains("host"));
        let back = scenario_from_json(&json).unwrap();
        assert_eq!(back.name, "loopback");
        assert_eq!(back.sessions[0].config.role, Role::Host);

        let path = tmp_path("demo.secs-scenario.json");
        write_scenario_file(&path, &sc).unwrap();
        let file_back = read_scenario_file(&path).unwrap();
        assert_eq!(file_back.sessions.len(), 1);
        let _ = fs::remove_file(&path);
    }
}
