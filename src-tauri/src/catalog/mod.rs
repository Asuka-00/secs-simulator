//! Prefabricated SECS message catalog (SMD import + per-message AutoReply).
//! 预制 SECS 消息目录（SMD 导入 + 按消息 AutoReply）。

mod body_tree;
mod data_item;
mod smd;

use std::sync::{Arc, Mutex};

use secs4rs::hsms::HsmsMessage;
use secs4rs::hsms_ss::HsmsSsCommunicator;
use secs4rs::secs2::Secs2;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppResult;
use crate::session::config::Role;
use body_tree::{body_tree_to_secs2, BodyItem};
use data_item::body_tree_to_sml;
use smd::parse_smd_xml;

/// Shared live catalog for open runtime (hot-updatable).
pub type SharedCatalog = Arc<Mutex<MessageCatalog>>;

pub fn new_shared_catalog(catalog: MessageCatalog) -> SharedCatalog {
    Arc::new(Mutex::new(catalog))
}

/// One prefabricated message (from SMD or hand-edited).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrefabMessage {
    pub id: String,
    pub message_name: String,
    pub description: String,
    /// Pair group, e.g. `S1F1` / `S1F13`.
    pub pair_name: String,
    pub stream: i32,
    pub function: i32,
    /// `H->E` or `H<-E`.
    pub direction: String,
    pub wait: bool,
    pub auto_reply: bool,
    pub no_logging: bool,
    /// SML-style body (legacy / fallback). Empty string = empty body.
    pub body_sml: String,
    /// Structured body tree (preferred for UI editor; preserves ItemName).
    #[serde(default)]
    pub body_tree: Vec<BodyItem>,
}

impl PrefabMessage {
    pub fn sx_fy(&self) -> String {
        format!("S{}F{}", self.stream, self.function)
    }

    pub fn is_host_to_equip(&self) -> bool {
        self.direction.contains("H->E") || self.direction.contains("H-&gt;E")
    }

    pub fn is_equip_to_host(&self) -> bool {
        self.direction.contains("H<-E") || self.direction.contains("H-&lt;E")
    }

    /// Direction this role **receives**.
    pub fn is_inbound_for(&self, role: &Role) -> bool {
        match role {
            Role::Equipment => self.is_host_to_equip(),
            Role::Host => self.is_equip_to_host(),
        }
    }

    /// Direction this role **sends**.
    pub fn is_outbound_for(&self, role: &Role) -> bool {
        match role {
            Role::Equipment => self.is_equip_to_host(),
            Role::Host => self.is_host_to_equip(),
        }
    }

    /// Prefer structured tree; fall back to SML.
    pub fn body_secs2(&self) -> AppResult<Secs2> {
        if !self.body_tree.is_empty() {
            return body_tree_to_secs2(&self.body_tree);
        }
        parse_body_sml(&self.body_sml)
    }

    /// Sync `body_sml` from tree (call after UI tree edits).
    pub fn sync_body_sml_from_tree(&mut self) {
        if self.body_tree.is_empty() {
            if self.body_sml.trim().is_empty() {
                return;
            }
            // Keep sml-only messages as-is.
            return;
        }
        self.body_sml = body_tree_to_sml(&self.body_tree);
    }
}

/// Per-session message library.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MessageCatalog {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub messages: Vec<PrefabMessage>,
}

impl MessageCatalog {
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&PrefabMessage> {
        self.messages.iter().find(|m| m.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut PrefabMessage> {
        self.messages.iter_mut().find(|m| m.id == id)
    }

    pub fn upsert(&mut self, msg: PrefabMessage) {
        if let Some(slot) = self.messages.iter_mut().find(|m| m.id == msg.id) {
            *slot = msg;
        } else {
            self.messages.push(msg);
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.messages.len();
        self.messages.retain(|m| m.id != id);
        self.messages.len() != before
    }

    /// Replace entire catalog from SMD XML text.
    pub fn import_smd(&mut self, xml: &str, source: impl Into<String>) -> AppResult<usize> {
        let parsed = parse_smd_xml(xml)?;
        let n = parsed.len();
        self.source = source.into();
        self.messages = parsed;
        Ok(n)
    }

    /// Find inbound primary with AutoReply for received SxFy.
    pub fn find_auto_reply_primary(
        &self,
        role: &Role,
        stream: i32,
        function: i32,
    ) -> Option<&PrefabMessage> {
        self.messages.iter().find(|m| {
            m.stream == stream
                && m.function == function
                && m.auto_reply
                && m.is_inbound_for(role)
        })
    }

    /// Find secondary reply for a primary (same pair + variant description, outbound).
    ///
    /// SMD reuses PairName for many S6F11 events; Description (ALARM_SET, …) is the
    /// variant key that pairs each S6F11 with its S6F12.
    pub fn find_reply_for(&self, role: &Role, primary: &PrefabMessage) -> Option<&PrefabMessage> {
        let reply_fn = if primary.function % 2 == 1 {
            primary.function + 1
        } else {
            return None;
        };
        let desc = primary.description.trim();
        // 1) Same pair + same Description (event variant) + outbound
        if !desc.is_empty() {
            if let Some(m) = self.messages.iter().find(|m| {
                m.pair_name == primary.pair_name
                    && m.stream == primary.stream
                    && m.function == reply_fn
                    && m.description.trim() == desc
                    && m.is_outbound_for(role)
            }) {
                return Some(m);
            }
        }
        // 2) Same pair + reply function + outbound
        self.messages
            .iter()
            .find(|m| {
                m.pair_name == primary.pair_name
                    && m.stream == primary.stream
                    && m.function == reply_fn
                    && m.is_outbound_for(role)
            })
            .or_else(|| {
                self.messages.iter().find(|m| {
                    m.stream == primary.stream
                        && m.function == reply_fn
                        && m.is_outbound_for(role)
                })
            })
    }
}

/// Parse body SML fragment into Secs2.
/// Accepts empty, full SML message, or bare body (`<L ...>` / `L:0` style from our formatter).
pub fn parse_body_sml(body: &str) -> AppResult<Secs2> {
    let t = body.trim();
    if t.is_empty() || t == "." {
        return Ok(Secs2::empty());
    }
    // Full message: SxFy ...
    if t.starts_with('S') || t.starts_with('s') {
        let msg = crate::sml_bridge::parse_sml(t)?;
        return Ok(msg.secs2().clone());
    }
    // Wrap bare body as S0F0 for secs4rs SML parser.
    let wrapped = if t.ends_with('.') {
        format!("S0F0 {t}")
    } else {
        format!("S0F0 {t}.")
    };
    let msg = crate::sml_bridge::parse_sml(&wrapped)?;
    Ok(msg.secs2().clone())
}

/// Try catalog auto-reply for a received DATA primary.
pub fn try_catalog_auto_reply(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    catalog: &MessageCatalog,
    role: &Role,
) -> Option<String> {
    if !primary.is_data_message() {
        return None;
    }
    // Only reply to W-bit primaries (or odd functions that expect reply).
    if !primary.wbit() && primary.get_function() % 2 == 0 {
        return None;
    }

    let stream = primary.get_stream();
    let function = primary.get_function();
    let req = catalog.find_auto_reply_primary(role, stream, function)?;
    let reply = catalog.find_reply_for(role, req)?;
    let body = match reply.body_secs2() {
        Ok(b) => b,
        Err(_) => return None,
    };
    match comm.send_data_reply(primary, reply.stream, reply.function, false, body) {
        Ok(()) => Some(format!(
            "auto-reply {} → {} ({})",
            req.sx_fy(),
            reply.sx_fy(),
            reply.message_name
        )),
        Err(_) => None,
    }
}

/// Build a new empty editable message.
pub fn new_blank_message() -> PrefabMessage {
    PrefabMessage {
        id: Uuid::new_v4().to_string(),
        message_name: "NewMessage".into(),
        description: String::new(),
        pair_name: "S1F1".into(),
        stream: 1,
        function: 1,
        direction: "H->E".into(),
        wait: true,
        auto_reply: false,
        no_logging: false,
        body_sml: String::new(),
        body_tree: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_k_erack_sample_header() {
        let xml = r#"<?xml version="1.0"?>
<SECSMessage>
  <AreYouThere>
    <Header>
      <MessageName>AreYouThere</MessageName>
      <Description PairName="S1F1">Are You There Request</Description>
      <Stream>1</Stream>
      <Function>1</Function>
      <Direction>H-&gt;E</Direction>
      <Wait>True</Wait>
      <AutoReply>True</AutoReply>
      <NoLogging>False</NoLogging>
    </Header>
    <DataItem />
  </AreYouThere>
  <OnLineData>
    <Header>
      <MessageName>OnLineData</MessageName>
      <Description PairName="S1F1">Are You There Request</Description>
      <Stream>1</Stream>
      <Function>2</Function>
      <Direction>H&lt;-E</Direction>
      <Wait>False</Wait>
      <AutoReply>False</AutoReply>
      <NoLogging>False</NoLogging>
    </Header>
    <DataItem>
      <L Count="2" Fixed="True" ItemName="">
        <A Count="6" Fixed="True" ItemName="MDLN">AIMFAb</A>
        <A Count="6" Fixed="True" ItemName="SOFTREV">V01R01</A>
      </L>
    </DataItem>
  </OnLineData>
</SECSMessage>"#;
        let mut cat = MessageCatalog::default();
        let n = cat.import_smd(xml, "test.smd").unwrap();
        assert_eq!(n, 2);
        assert!(cat.messages[0].auto_reply);
        assert!(cat.messages[0].wait);
        assert_eq!(cat.messages[0].stream, 1);
        assert_eq!(cat.messages[0].function, 1);
        assert!(cat.messages[0].is_host_to_equip());

        let reply = cat.find_reply_for(&Role::Equipment, &cat.messages[0]).unwrap();
        assert_eq!(reply.function, 2);
        assert!(reply.is_equip_to_host());
        assert!(!reply.body_tree.is_empty());
        assert_eq!(reply.body_tree[0].item_type, "L");
        assert_eq!(reply.body_tree[0].children[0].name, "MDLN");
        assert_eq!(reply.body_tree[0].children[0].value, "AIMFAb");
        let body = reply.body_secs2().unwrap();
        assert_eq!(body.get_ascii_at(&[0]).unwrap(), "AIMFAb");
        assert_eq!(body.get_ascii_at(&[1]).unwrap(), "V01R01");
    }

    #[test]
    fn import_real_k_erack_if_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../K-ERACK.SMD");
        if !path.is_file() {
            return;
        }
        let xml = std::fs::read_to_string(&path).unwrap();
        let mut cat = MessageCatalog::default();
        let n = cat.import_smd(&xml, "K-ERACK.SMD").unwrap();
        assert!(n > 100, "expected many messages, got {n}");
        // Equip receives S1F13 H->E with AutoReply
        let p = cat
            .find_auto_reply_primary(&Role::Equipment, 1, 13)
            .expect("S1F13 auto primary for equip");
        let r = cat.find_reply_for(&Role::Equipment, p).unwrap();
        assert_eq!(r.function, 14);

        // Many S6F11 event variants: Description is the title, not all "OFFLINE"
        let s6f11: Vec<_> = cat
            .messages
            .iter()
            .filter(|m| m.stream == 6 && m.function == 11)
            .collect();
        assert!(s6f11.len() > 10, "expected many S6F11 variants, got {}", s6f11.len());
        let names: std::collections::HashSet<_> =
            s6f11.iter().map(|m| m.message_name.as_str()).collect();
        assert!(
            names.contains("ALARM_SET") || names.contains("CARRIER_ID_ERROR"),
            "S6F11 variants should use Description as name, got {:?}",
            names.iter().take(8).collect::<Vec<_>>()
        );
        assert!(
            !names.iter().all(|n| *n == "S6F11" || *n == "OFFLINE"),
            "all S6F11 collapsed to one name: {:?}",
            names
        );
        // S6F12 reply for ALARM_SET must pair by Description, not first OFFLINE
        let alarm = s6f11
            .iter()
            .find(|m| m.description == "ALARM_SET" || m.message_name == "ALARM_SET")
            .expect("ALARM_SET S6F11");
        let reply = cat
            .find_reply_for(&Role::Host, alarm)
            .expect("S6F12 for ALARM_SET");
        assert_eq!(reply.function, 12);
        assert_eq!(reply.description, alarm.description);
    }

    #[test]
    fn doc1_s6f11_empty_ascii_encodes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../doc1.SMD");
        if !path.is_file() {
            return;
        }
        let xml = std::fs::read_to_string(&path).unwrap();
        let mut cat = MessageCatalog::default();
        cat.import_smd(&xml, "doc1.SMD").unwrap();
        let m = cat
            .messages
            .iter()
            .find(|m| m.stream == 6 && m.function == 11)
            .expect("S6F11");
        let body = m.body_secs2().expect("empty <A Count=0> must encode");
        assert!(!body.is_empty_item());
        // Equip catalog must not AR this primary (H<-E); Host can.
        assert!(cat
            .find_auto_reply_primary(&Role::Equipment, 6, 11)
            .is_none());
        assert!(cat.find_auto_reply_primary(&Role::Host, 6, 11).is_some());
    }
}
