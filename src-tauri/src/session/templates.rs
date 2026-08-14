//! Quick-start session configuration templates.

use serde::{Deserialize, Serialize};

use crate::session::config::{ConnectionMode, Role, SessionConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config: SessionConfig,
}

/// Built-in templates for common lab setups.
pub fn builtin_templates() -> Vec<SessionTemplate> {
    vec![
        SessionTemplate {
            id: "equip-passive-5000".into(),
            name: "Equip Passive :5000".into(),
            description: "Equipment role, Passive listen on 127.0.0.1:5000, sessionId=10".into(),
            config: SessionConfig {
                name: "Equip".into(),
                role: Role::Equipment,
                mode: ConnectionMode::Passive,
                ip: "127.0.0.1".into(),
                port: 5000,
                session_id: 10,
                linktest_enabled: false,
                rebind_if_passive: true,
                mdln: "SECS-SIM".into(),
                softrev: "0.1.0".into(),
                ..SessionConfig::default()
            },
        },
        SessionTemplate {
            id: "host-active-5000".into(),
            name: "Host Active :5000".into(),
            description: "Host role, Active connect to 127.0.0.1:5000, sessionId=10".into(),
            config: SessionConfig {
                name: "Host".into(),
                role: Role::Host,
                mode: ConnectionMode::Active,
                ip: "127.0.0.1".into(),
                port: 5000,
                session_id: 10,
                linktest_enabled: false,
                rebind_if_passive: false,
                ..SessionConfig::default()
            },
        },
        SessionTemplate {
            id: "equip-passive-5001".into(),
            name: "Equip Passive :5001".into(),
            description: "Second equipment on port 5001 (multi-session)".into(),
            config: SessionConfig {
                name: "Equip-2".into(),
                role: Role::Equipment,
                mode: ConnectionMode::Passive,
                ip: "127.0.0.1".into(),
                port: 5001,
                session_id: 11,
                linktest_enabled: false,
                ..SessionConfig::default()
            },
        },
        SessionTemplate {
            id: "host-active-5001".into(),
            name: "Host Active :5001".into(),
            description: "Host Active to port 5001".into(),
            config: SessionConfig {
                name: "Host-2".into(),
                role: Role::Host,
                mode: ConnectionMode::Active,
                ip: "127.0.0.1".into(),
                port: 5001,
                session_id: 11,
                linktest_enabled: false,
                rebind_if_passive: false,
                ..SessionConfig::default()
            },
        },
    ]
}

/// Scenario-style pair: Equip Passive + Host Active on same port.
pub fn loopback_pair_templates(port: u16) -> Vec<SessionConfig> {
    let sid = 10i32;
    vec![
        SessionConfig {
            name: "Equip".into(),
            role: Role::Equipment,
            mode: ConnectionMode::Passive,
            ip: "127.0.0.1".into(),
            port,
            session_id: sid,
            linktest_enabled: false,
            rebind_if_passive: true,
            mdln: "SECS-SIM".into(),
            softrev: "0.1.0".into(),
            ..SessionConfig::default()
        },
        SessionConfig {
            name: "Host".into(),
            role: Role::Host,
            mode: ConnectionMode::Active,
            ip: "127.0.0.1".into(),
            port,
            session_id: sid,
            linktest_enabled: false,
            rebind_if_passive: false,
            ..SessionConfig::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_non_empty() {
        let t = builtin_templates();
        assert!(t.len() >= 4);
        assert!(t.iter().any(|x| x.config.role == Role::Equipment));
        assert!(t.iter().any(|x| x.config.role == Role::Host));
        let pair = loopback_pair_templates(5000);
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0].port, pair[1].port);
    }
}
