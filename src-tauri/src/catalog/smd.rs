//! Parse GWGEM-style `.SMD` (SECSMessage XML) into prefab messages.
//! 解析 GWGEM 风格 `.SMD`（SECSMessage XML）为预制消息。

use roxmltree::{Document, Node};
use uuid::Uuid;

use super::data_item::{body_tree_to_sml, data_item_node_to_tree};
use super::PrefabMessage;
use crate::error::{AppError, AppResult};

pub fn parse_smd_file(path: &std::path::Path) -> AppResult<Vec<PrefabMessage>> {
    let xml = std::fs::read_to_string(path)
        .map_err(|e| AppError::Message(format!("read SMD failed: {e}")))?;
    parse_smd_xml(&xml)
}

pub fn parse_smd_xml(xml: &str) -> AppResult<Vec<PrefabMessage>> {
    let doc = Document::parse(xml)
        .map_err(|e| AppError::Message(format!("SMD XML parse error: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "SECSMessage" {
        return Err(AppError::Message(format!(
            "expected root <SECSMessage>, got <{}>",
            root.tag_name().name()
        )));
    }

    let mut out = Vec::new();
    for child in root.children().filter(|n| n.is_element()) {
        if let Some(msg) = parse_message_element(child)? {
            out.push(msg);
        }
    }
    Ok(out)
}

fn parse_message_element(node: Node<'_, '_>) -> AppResult<Option<PrefabMessage>> {
    let header = match node.children().find(|n| n.is_element() && n.tag_name().name() == "Header")
    {
        Some(h) => h,
        None => return Ok(None),
    };

    let raw_message_name = text_of(header, "MessageName")
        .unwrap_or_else(|| node.tag_name().name().to_string());
    let desc_node = header
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Description");
    let description = desc_node
        .and_then(|n| n.text())
        .unwrap_or("")
        .trim()
        .to_string();
    let pair_name = desc_node
        .and_then(|n| n.attribute("PairName"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let s = text_of(header, "Stream").unwrap_or_else(|| "0".into());
            let f = text_of(header, "Function").unwrap_or_else(|| "0".into());
            format!("S{s}F{f}")
        });
    // SMD often sets MessageName to bare "S6F11" and puts the real title in
    // Description (ALARM_SET, CARRIER_ID_ERROR, …). Prefer Description for UI name.
    let message_name = {
        let sx_like = raw_message_name.eq_ignore_ascii_case(&pair_name)
            || {
                let re_ok = raw_message_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric())
                    && raw_message_name.to_ascii_uppercase().starts_with('S')
                    && raw_message_name.contains('F');
                re_ok && description.len() > 0 && description != raw_message_name
            };
        if sx_like && !description.is_empty() {
            description.clone()
        } else {
            raw_message_name
        }
    };

    let stream: i32 = text_of(header, "Stream")
        .ok_or_else(|| AppError::Message("missing Stream".into()))?
        .parse()
        .map_err(|_| AppError::Message("invalid Stream".into()))?;
    let function: i32 = text_of(header, "Function")
        .ok_or_else(|| AppError::Message("missing Function".into()))?
        .parse()
        .map_err(|_| AppError::Message("invalid Function".into()))?;

    let direction_raw = text_of(header, "Direction").unwrap_or_default();
    let direction = normalize_direction(&direction_raw);
    let wait = parse_bool(&text_of(header, "Wait").unwrap_or_else(|| "False".into()));
    let auto_reply = parse_bool(&text_of(header, "AutoReply").unwrap_or_else(|| "False".into()));
    let no_logging = parse_bool(&text_of(header, "NoLogging").unwrap_or_else(|| "False".into()));

    let (body_tree, body_sml) = if let Some(di) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "DataItem")
    {
        let tree = data_item_node_to_tree(di)?;
        let sml = body_tree_to_sml(&tree);
        (tree, sml)
    } else {
        (vec![], String::new())
    };

    Ok(Some(PrefabMessage {
        id: Uuid::new_v4().to_string(),
        message_name,
        description,
        pair_name,
        stream,
        function,
        direction,
        wait,
        auto_reply,
        no_logging,
        body_sml,
        body_tree,
    }))
}

fn text_of(parent: Node<'_, '_>, tag: &str) -> Option<String> {
    parent
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == tag)
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn normalize_direction(raw: &str) -> String {
    // XML may decode entities already; keep compact form.
    let s = raw
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace(" ", "");
    if s.contains("H->E") || s == "H->E" {
        "H->E".into()
    } else if s.contains("H<-E") || s == "H<-E" {
        "H<-E".into()
    } else {
        raw.trim().to_string()
    }
}
