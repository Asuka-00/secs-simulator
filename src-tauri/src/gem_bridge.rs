//! Builtin GEM auto-reply handlers (secs4rs::gem free functions).

use std::sync::{Arc, Mutex};

use secs4rs::gem::{
    s1f14, s1f16, s1f18, s1f2, s2f18, s5f2, s6f12, Ackc5, Ackc6, Clock, ClockType as GemClockType,
    CommAck, GemConfig, LocalDateTime, OnlAck,
};
use secs4rs::hsms::HsmsMessage;
use secs4rs::hsms_ss::HsmsSsCommunicator;
use serde::{Deserialize, Serialize};

use crate::session::config::{ClockType, Role, SessionConfig};

/// Per-session toggles for builtin auto-replies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinHandlers {
    /// Equip: S1F13 → S1F14
    pub s1f13: bool,
    /// Equip: S1F17 → S1F18
    pub s1f17: bool,
    /// Equip: S1F15 → S1F16
    pub s1f15: bool,
    /// Equip: S1F1 → S1F2
    pub s1f1: bool,
    /// Equip: S2F17 → S2F18
    pub s2f17: bool,
    /// Host/Equip: S6F11 → S6F12
    pub s6f11: bool,
    /// Host/Equip: S5F1 → S5F2
    pub s5f1: bool,
}

impl BuiltinHandlers {
    pub fn for_role(role: &Role) -> Self {
        let equip = matches!(role, Role::Equipment);
        Self {
            s1f13: equip,
            s1f17: equip,
            s1f15: equip,
            s1f1: equip,
            s2f17: equip,
            s6f11: true,
            s5f1: true,
        }
    }
}

impl Default for BuiltinHandlers {
    fn default() -> Self {
        Self::for_role(&Role::Equipment)
    }
}

/// Build secs4rs GemConfig from session connection config.
pub fn gem_config_from_session(cfg: &SessionConfig) -> Arc<GemConfig> {
    let g = GemConfig::new();
    g.set_is_equip(matches!(cfg.role, Role::Equipment));
    g.set_mdln(cfg.mdln.clone());
    g.set_softrev(cfg.softrev.clone());
    g.set_clock_type(match cfg.clock_type {
        ClockType::A12 => GemClockType::A12,
        ClockType::A16 => GemClockType::A16,
    });
    Arc::new(g)
}

fn now_clock() -> Clock {
    // Simulator clock: wall-ish UTC via crude day count; good enough for S2F18 body.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let tod = secs % 86400;
    let hour = (tod / 3600) as u32;
    let minute = ((tod % 3600) / 60) as u32;
    let second = (tod % 60) as u32;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Clock::from_local(LocalDateTime::new(
        y as i32,
        m as u32,
        d as u32,
        hour,
        minute,
        second,
    ))
}

/// Try to auto-reply a primary DATA. Returns handler id if handled.
pub fn try_auto_reply(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    handlers: &BuiltinHandlers,
    gem: &GemConfig,
) -> Option<&'static str> {
    if !primary.wbit() || !primary.is_data_message() {
        return None;
    }
    let s = primary.get_stream();
    let f = primary.get_function();
    let equip = gem.is_equip();

    match (s, f) {
        (1, 13) if handlers.s1f13 && equip => s1f14(comm, primary, gem, CommAck::Ok)
            .ok()
            .filter(|x| *x)
            .map(|_| "s1f14"),
        (1, 17) if handlers.s1f17 && equip => s1f18(comm, primary, OnlAck::Ok)
            .ok()
            .filter(|x| *x)
            .map(|_| "s1f18"),
        (1, 15) if handlers.s1f15 && equip => {
            s1f16(comm, primary).ok().filter(|x| *x).map(|_| "s1f16")
        }
        (1, 1) if handlers.s1f1 && equip => {
            s1f2(comm, primary, gem).ok().filter(|x| *x).map(|_| "s1f2")
        }
        (2, 17) if handlers.s2f17 && equip => {
            let clock = now_clock();
            s2f18(comm, primary, gem, &clock)
                .ok()
                .filter(|x| *x)
                .map(|_| "s2f18")
        }
        (6, 11) if handlers.s6f11 => s6f12(comm, primary, Ackc6::Ok)
            .ok()
            .filter(|x| *x)
            .map(|_| "s6f12"),
        (5, 1) if handlers.s5f1 => s5f2(comm, primary, Ackc5::Ok)
            .ok()
            .filter(|x| *x)
            .map(|_| "s5f2"),
        _ => None,
    }
}

/// Shared live GEM state for an open session.
pub struct LiveGem {
    pub config: Arc<GemConfig>,
    pub handlers: Arc<Mutex<BuiltinHandlers>>,
}

impl LiveGem {
    pub fn new(cfg: &SessionConfig, handlers: BuiltinHandlers) -> Self {
        Self {
            config: gem_config_from_session(cfg),
            handlers: Arc::new(Mutex::new(handlers)),
        }
    }
}
