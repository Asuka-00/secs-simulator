//! Ring-buffer message log per session.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use secs4rs::hsms::{encode_frame, HsmsMessage};
use secs4rs::secs2::Secs2;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Default capacity (plan default).
pub const DEFAULT_LOG_CAPACITY: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogDirection {
    Tx,
    Rx,
    PassThrough,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub direction: LogDirection,
    pub stream: Option<u8>,
    pub function: Option<u8>,
    pub wbit: Option<bool>,
    pub system_bytes: Option<u32>,
    pub session: Option<i32>,
    pub summary: String,
    pub sml: Option<String>,
    pub hex: Option<String>,
}

impl LogEntry {
    pub fn system(summary: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp_ms: now_ms(),
            direction: LogDirection::System,
            stream: None,
            function: None,
            wbit: None,
            system_bytes: None,
            session: None,
            summary: summary.into(),
            sml: None,
            hex: None,
        }
    }

    pub fn from_hsms(msg: &HsmsMessage, direction: LogDirection) -> Self {
        let is_data = msg.is_data_message();
        let (stream, function, wbit) = if is_data {
            (
                Some(msg.get_stream() as u8),
                Some(msg.get_function() as u8),
                Some(msg.wbit()),
            )
        } else {
            (None, None, None)
        };

        let summary = if is_data {
            let w = if msg.wbit() { " W" } else { "" };
            format!("S{}F{}{w}", msg.get_stream(), msg.get_function())
        } else {
            msg.message_type().name().to_string()
        };

        let sml = if is_data {
            Some(format_sml_like(msg.get_stream(), msg.get_function(), msg.wbit(), msg.secs2()))
        } else {
            None
        };

        let hex = encode_frame(msg)
            .ok()
            .map(|b| hex_encode(&b));

        Self {
            id: Uuid::new_v4().to_string(),
            timestamp_ms: now_ms(),
            direction,
            stream,
            function,
            wbit,
            system_bytes: Some(msg.system_bytes_key() as u32),
            session: Some(msg.session_id()),
            summary,
            sml,
            hex,
        }
    }
}

/// Bounded ring buffer of log entries.
#[derive(Debug, Clone)]
pub struct SessionLog {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl Default for SessionLog {
    fn default() -> Self {
        Self::new(DEFAULT_LOG_CAPACITY)
    }
}

impl SessionLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(1024)),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Count DATA messages matching stream/function in either direction.
    pub fn count_data_sf(&self, stream: u8, function: u8) -> usize {
        self.entries
            .iter()
            .filter(|e| e.stream == Some(stream) && e.function == Some(function))
            .count()
    }

    pub fn count_direction(&self, dir: LogDirection) -> usize {
        self.entries.iter().filter(|e| e.direction == dir).count()
    }
}

/// Export format for session logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogExportFormat {
    Json,
    Text,
}

fn dir_label(d: &LogDirection) -> &'static str {
    match d {
        LogDirection::Tx => "TX",
        LogDirection::Rx => "RX",
        LogDirection::PassThrough => "PT",
        LogDirection::System => "SYS",
    }
}

/// Format log entries for export (JSON array or line-oriented text).
pub fn export_logs(entries: &[LogEntry], format: LogExportFormat) -> Result<String, String> {
    match format {
        LogExportFormat::Json => serde_json::to_string_pretty(entries)
            .map_err(|e| format!("json export: {e}")),
        LogExportFormat::Text => {
            let mut out = String::new();
            out.push_str("# SECS Simulator log export\n");
            out.push_str("# time\tdir\tsummary\tsys\tsession\tsml\thex\n");
            for e in entries {
                let t = e.timestamp_ms;
                let sml = e.sml.as_deref().unwrap_or("").replace('\n', "\\n");
                let hex = e.hex.as_deref().unwrap_or("");
                out.push_str(&format!(
                    "{t}\t{}\t{}\t{}\t{}\t{sml}\t{hex}\n",
                    dir_label(&e.direction),
                    e.summary.replace('\t', " "),
                    e.system_bytes.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                    e.session.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                ));
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn export_text_and_json() {
        let mut log = SessionLog::new(10);
        log.push(LogEntry::system("hello"));
        let entries = log.entries();
        let text = export_logs(&entries, LogExportFormat::Text).unwrap();
        assert!(text.contains("SYS"));
        assert!(text.contains("hello"));
        let json = export_logs(&entries, LogExportFormat::Json).unwrap();
        assert!(json.contains("hello"));
        assert!(json.contains("system") || json.contains("System"));
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

/// Lightweight SML-ish dump for log panel (not a full SML round-trip).
fn format_sml_like(stream: i32, function: i32, wbit: bool, body: &Secs2) -> String {
    let w = if wbit { " W" } else { "" };
    format!("S{stream}F{function}{w}\n{}", format_secs2(body, 0))
}

fn format_secs2(item: &Secs2, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match item {
        Secs2::List(items) => {
            if items.is_empty() {
                format!("{pad}<L[0]>")
            } else {
                let mut s = format!("{pad}<L[{}]>\n", items.len());
                for (i, c) in items.iter().enumerate() {
                    s.push_str(&format_secs2(c, indent + 1));
                    if i + 1 < items.len() {
                        s.push('\n');
                    }
                }
                s
            }
        }
        Secs2::Ascii(a) => format!("{pad}<A[{}] \"{}\">", a.len(), a.escape_default()),
        Secs2::Binary(b) => format!("{pad}<B[{}] {}>", b.len(), hex_encode(b)),
        Secs2::Boolean(v) => {
            let bits: Vec<&str> = v.iter().map(|x| if *x { "T" } else { "F" }).collect();
            format!("{pad}<BOOLEAN[{}] {}>", v.len(), bits.join(" "))
        }
        Secs2::Int1(v) => format!("{pad}<I1[{}] {:?}>", v.len(), v),
        Secs2::Int2(v) => format!("{pad}<I2[{}] {:?}>", v.len(), v),
        Secs2::Int4(v) => format!("{pad}<I4[{}] {:?}>", v.len(), v),
        Secs2::Int8(v) => format!("{pad}<I8[{}] {:?}>", v.len(), v),
        Secs2::Uint1(v) => format!("{pad}<U1[{}] {:?}>", v.len(), v),
        Secs2::Uint2(v) => format!("{pad}<U2[{}] {:?}>", v.len(), v),
        Secs2::Uint4(v) => format!("{pad}<U4[{}] {:?}>", v.len(), v),
        Secs2::Uint8(v) => format!("{pad}<U8[{}] {:?}>", v.len(), v),
        Secs2::Float4(v) => format!("{pad}<F4[{}] {:?}>", v.len(), v),
        Secs2::Float8(v) => format!("{pad}<F8[{}] {:?}>", v.len(), v),
        Secs2::Jis8(v) => format!("{pad}<J[{}] {}>", v.len(), hex_encode(v)),
        Secs2::Unicode(v) => format!("{pad}<U2CHAR[{}] {}>", v.len(), hex_encode(v)),
        Secs2::Empty => format!("{pad}<EMPTY>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut log = SessionLog::new(3);
        for i in 0..5 {
            log.push(LogEntry::system(format!("n{i}")));
        }
        assert_eq!(log.len(), 3);
        let entries = log.entries();
        assert_eq!(entries[0].summary, "n2");
        assert_eq!(entries[2].summary, "n4");
    }
}
