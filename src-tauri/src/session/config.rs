//! Per-session connection configuration DTO.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Host,
    Equipment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    Active,
    Passive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClockType {
    A12,
    A16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    pub name: String,
    pub role: Role,
    pub mode: ConnectionMode,
    pub ip: String,
    pub port: u16,
    pub session_id: i32,
    pub t3: f32,
    pub t5: f32,
    pub t6: f32,
    pub t7: f32,
    pub t8: f32,
    pub linktest_enabled: bool,
    pub linktest_seconds: f32,
    pub rebind_if_passive: bool,
    pub mdln: String,
    pub softrev: String,
    pub clock_type: ClockType,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            name: "Session".into(),
            role: Role::Equipment,
            mode: ConnectionMode::Passive,
            ip: "127.0.0.1".into(),
            port: 5000,
            session_id: 0,
            t3: 45.0,
            t5: 10.0,
            t6: 5.0,
            t7: 10.0,
            t8: 5.0,
            linktest_enabled: false,
            linktest_seconds: 30.0,
            rebind_if_passive: true,
            mdln: "SECS-SIM".into(),
            softrev: "0.1.0".into(),
            clock_type: ClockType::A16,
        }
    }
}
