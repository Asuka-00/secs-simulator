//! Declarative auto-reply rule matching & execution.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use secs4rs::gem::{
    s1f14, s1f16, s1f18, s1f2, s2f18, s5f2, s6f12, Ackc5, Ackc6, Clock, CommAck, GemConfig,
    LocalDateTime, OnlAck,
};
use secs4rs::hsms::HsmsMessage;
use secs4rs::hsms_ss::HsmsSsCommunicator;
use secs4rs::secs2::Secs2;
use secs4rs::sml::SmlMessage;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Match fields; `None` = wildcard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleMatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wbit: Option<bool>,
}

impl RuleMatch {
    fn matches(&self, msg: &HsmsMessage) -> bool {
        if !msg.is_data_message() {
            return false;
        }
        if let Some(s) = self.stream {
            if msg.get_stream() as u8 != s {
                return false;
            }
        }
        if let Some(f) = self.function {
            if msg.get_function() as u8 != f {
                return false;
            }
        }
        if let Some(w) = self.wbit {
            if msg.wbit() != w {
                return false;
            }
        }
        true
    }

    /// Higher = more specific (exact before wildcards).
    fn specificity(&self) -> u8 {
        let mut n = 0u8;
        if self.stream.is_some() {
            n += 2;
        }
        if self.function.is_some() {
            n += 2;
        }
        if self.wbit.is_some() {
            n += 1;
        }
        n
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuleAction {
    #[serde(rename = "builtin")]
    Builtin {
        handler: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<serde_json::Value>,
    },
    #[serde(rename = "sml_reply")]
    SmlReply {
        /// Body item SML (`<L …>`) or full `SxFy … .` (stream/func ignored for reply header).
        body: String,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    #[serde(rename = "sml_primary")]
    SmlPrimary {
        sml: String,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    #[serde(rename = "drop")]
    Drop,
    #[serde(rename = "log_only")]
    LogOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(rename = "match")]
    pub match_spec: RuleMatch,
    pub action: RuleAction,
    /// If true, keep evaluating lower-specificity rules after this hit.
    #[serde(default)]
    pub continue_match: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSet {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn default_version() -> u32 {
    1
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            version: 1,
            rules: Vec::new(),
        }
    }
}

/// Outcome of evaluating rules against one primary.
#[derive(Debug, Clone)]
pub enum RuleOutcome {
    /// No rule matched.
    None,
    /// Matched; do not run M5 builtin fallback.
    Handled { rule_id: String, note: String },
    /// Matched drop — suppress builtin auto-reply.
    Dropped { rule_id: String },
}

/// Shared live rules for open session.
pub type SharedRules = Arc<Mutex<RuleSet>>;

pub fn new_shared_rules(set: RuleSet) -> SharedRules {
    Arc::new(Mutex::new(set))
}

/// Parse SML body item: accepts full `SxFy … .` or bare `<…>`.
pub fn parse_secs2_body(text: &str) -> AppResult<Secs2> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Secs2::empty());
    }
    let wrapped = if t.starts_with('<') {
        // Body-only: wrap as dummy message for secs4rs parser.
        format!("S0F0 {t}.")
    } else if t.ends_with('.') {
        t.to_string()
    } else {
        format!("{t}.")
    };
    let m = SmlMessage::of(&wrapped).map_err(|e| AppError::Message(format!("rule body SML: {e}")))?;
    Ok(m.secs2().clone())
}

fn sleep_ms(ms: u64) {
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}

fn now_clock() -> Clock {
    Clock::from_local(LocalDateTime::new(2026, 8, 6, 12, 0, 0))
}

fn run_builtin(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    handler: &str,
    gem: &GemConfig,
) -> AppResult<bool> {
    let h = handler.to_ascii_lowercase();
    let ok = match h.as_str() {
        "s1f14" | "s1f13" => s1f14(comm, primary, gem, CommAck::Ok).map_err(|e| {
            AppError::Message(format!("builtin {handler}: {e}"))
        })?,
        "s1f18" | "s1f17" => s1f18(comm, primary, OnlAck::Ok)
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        "s1f16" | "s1f15" => s1f16(comm, primary)
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        "s1f2" | "s1f1" => s1f2(comm, primary, gem)
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        "s2f18" | "s2f17" => s2f18(comm, primary, gem, &now_clock())
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        "s6f12" | "s6f11" => s6f12(comm, primary, Ackc6::Ok)
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        "s5f2" | "s5f1" => s5f2(comm, primary, Ackc5::Ok)
            .map_err(|e| AppError::Message(format!("builtin {handler}: {e}")))?,
        other => {
            return Err(AppError::Message(format!(
                "unknown builtin handler: {other}"
            )))
        }
    };
    Ok(ok)
}

/// Evaluate enabled matching rules (specific first). Execute until stop.
pub fn evaluate_and_apply(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    rules: &RuleSet,
    gem: &GemConfig,
) -> RuleOutcome {
    let mut candidates: Vec<(usize, u8, &Rule)> = rules
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled && r.match_spec.matches(primary))
        .map(|(i, r)| (i, r.match_spec.specificity(), r))
        .collect();

    // Specificity desc, then original order asc.
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    if candidates.is_empty() {
        return RuleOutcome::None;
    }

    let mut last_note = String::new();
    let mut last_id = String::new();
    let mut any_handled = false;
    let mut dropped = false;

    for (_, _, rule) in candidates {
        last_id = rule.id.clone();
        match &rule.action {
            RuleAction::Drop => {
                dropped = true;
                last_note = "drop".into();
                any_handled = true;
                if !rule.continue_match {
                    break;
                }
            }
            RuleAction::LogOnly => {
                last_note = "log_only".into();
                any_handled = true;
                if !rule.continue_match {
                    // log_only still suppresses builtin when stop
                    break;
                }
            }
            RuleAction::SmlReply { body, delay_ms } => {
                sleep_ms(*delay_ms);
                if !primary.wbit() {
                    last_note = "sml_reply skipped (no W-bit)".into();
                } else {
                    match parse_secs2_body(body) {
                        Ok(secs2) => {
                            let stream = primary.get_stream();
                            let func = primary.get_function().saturating_add(1);
                            match comm.send_data_reply(primary, stream, func, false, secs2) {
                                Ok(()) => {
                                    last_note = format!("sml_reply S{stream}F{func}");
                                    any_handled = true;
                                }
                                Err(e) => {
                                    last_note = format!("sml_reply error: {e}");
                                    any_handled = true;
                                }
                            }
                        }
                        Err(e) => {
                            last_note = format!("sml_reply parse: {e}");
                            any_handled = true;
                        }
                    }
                }
                if !rule.continue_match {
                    break;
                }
            }
            RuleAction::SmlPrimary { sml, delay_ms } => {
                sleep_ms(*delay_ms);
                match SmlMessage::of(sml.trim()) {
                    Ok(m) => {
                        match comm.send_data(
                            m.get_stream(),
                            m.get_function(),
                            m.wbit(),
                            m.secs2().clone(),
                        ) {
                            Ok(_) => {
                                last_note = format!(
                                    "sml_primary S{}F{}",
                                    m.get_stream(),
                                    m.get_function()
                                );
                                any_handled = true;
                            }
                            Err(e) => {
                                last_note = format!("sml_primary error: {e}");
                                any_handled = true;
                            }
                        }
                    }
                    Err(e) => {
                        last_note = format!("sml_primary parse: {e}");
                        any_handled = true;
                    }
                }
                if !rule.continue_match {
                    break;
                }
            }
            RuleAction::Builtin { handler, .. } => {
                match run_builtin(comm, primary, handler, gem) {
                    Ok(true) => {
                        last_note = format!("builtin:{handler}");
                        any_handled = true;
                    }
                    Ok(false) => {
                        last_note = format!("builtin:{handler} skipped");
                        any_handled = true;
                    }
                    Err(e) => {
                        last_note = format!("builtin:{handler} error: {e}");
                        any_handled = true;
                    }
                }
                if !rule.continue_match {
                    break;
                }
            }
        }
    }

    if dropped {
        RuleOutcome::Dropped { rule_id: last_id }
    } else if any_handled {
        RuleOutcome::Handled {
            rule_id: last_id,
            note: last_note,
        }
    } else {
        RuleOutcome::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_header(stream: u8, function: u8, wbit: bool) -> HsmsMessage {
        let mut h = [0u8; 10];
        h[0] = 0x00;
        h[1] = 0x0A;
        h[2] = stream | if wbit { 0x80 } else { 0 };
        h[3] = function;
        // DATA p=0 s=0
        h[4] = 0;
        h[5] = 0;
        h[6] = 0;
        h[7] = 0;
        h[8] = 0;
        h[9] = 1;
        HsmsMessage::of_with_body(&h, Secs2::list_empty()).unwrap()
    }

    #[test]
    fn match_specificity_and_wildcard() {
        let msg = data_header(2, 41, true);
        let exact = RuleMatch {
            stream: Some(2),
            function: Some(41),
            wbit: Some(true),
        };
        let wild = RuleMatch {
            stream: Some(2),
            function: None,
            wbit: None,
        };
        assert!(exact.matches(&msg));
        assert!(wild.matches(&msg));
        assert!(exact.specificity() > wild.specificity());
    }

    #[test]
    fn parse_body_item() {
        let b = parse_secs2_body(r#"<L <B 0x00> <A "OK"> >"#).unwrap();
        assert_eq!(b.size(), 2);
    }
}
