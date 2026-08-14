//! SML parse / format helpers around `secs4rs::sml`.

use secs4rs::hsms::HsmsMessage;
use secs4rs::sml::SmlMessage;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::session::log::LogEntry;

/// Parsed SML header + body snapshot for UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSml {
    pub stream: i32,
    pub function: i32,
    pub wbit: bool,
    pub summary: String,
}

/// Result of sending an SML primary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendSmlResult {
    pub stream: i32,
    pub function: i32,
    pub wbit: bool,
    pub summary: String,
    /// Present when W-bit and a reply arrived within T3 (sync send only).
    pub reply: Option<ReplyInfo>,
    /// W-bit primary: reply will arrive later via `session-event` (`send_done` / `error`).
    #[serde(default)]
    pub waiting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyInfo {
    pub stream: i32,
    pub function: i32,
    pub summary: String,
    pub sml: Option<String>,
    pub hex: Option<String>,
}

pub fn parse_sml(text: &str) -> AppResult<SmlMessage> {
    SmlMessage::of(text.trim()).map_err(|e| AppError::Message(format!("SML: {e}")))
}

pub fn parsed_preview(text: &str) -> AppResult<ParsedSml> {
    let m = parse_sml(text)?;
    let w = if m.wbit() { " W" } else { "" };
    Ok(ParsedSml {
        stream: m.get_stream(),
        function: m.get_function(),
        wbit: m.wbit(),
        summary: format!("S{}F{}{w}", m.get_stream(), m.get_function()),
    })
}

pub fn reply_info(msg: &HsmsMessage) -> ReplyInfo {
    let entry = LogEntry::from_hsms(msg, crate::session::log::LogDirection::Rx);
    ReplyInfo {
        stream: msg.get_stream(),
        function: msg.get_function(),
        summary: entry.summary,
        sml: entry.sml,
        hex: entry.hex,
    }
}
