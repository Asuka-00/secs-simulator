//! SMD `<DataItem>` XML → `Secs2`, body tree, and SML body formatting.

use roxmltree::{Document, Node};
use secs4rs::secs2::Secs2;
use uuid::Uuid;

use super::body_tree::BodyItem;
use crate::error::{AppError, AppResult};

/// Parse a fragment like `<DataItem>...</DataItem>` or its inner children.
pub fn data_item_to_secs2(xml_fragment: &str) -> AppResult<Secs2> {
    let wrapped = if xml_fragment.trim_start().starts_with('<') {
        format!("<_r>{xml_fragment}</_r>")
    } else {
        return Ok(Secs2::empty());
    };
    let doc = Document::parse(&wrapped)
        .map_err(|e| AppError::Message(format!("DataItem XML error: {e}")))?;
    let root = doc.root_element();
    let di = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "DataItem")
        .unwrap_or(root);
    data_item_node_to_secs2(di)
}

pub fn data_item_node_to_secs2(di: Node<'_, '_>) -> AppResult<Secs2> {
    let tree = data_item_node_to_tree(di)?;
    super::body_tree::body_tree_to_secs2(&tree)
}

/// Parse DataItem XML into named body tree (preserves ItemName for UI).
pub fn data_item_node_to_tree(di: Node<'_, '_>) -> AppResult<Vec<BodyItem>> {
    let children: Vec<_> = di.children().filter(|n| n.is_element()).collect();
    if children.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::with_capacity(children.len());
    for c in children {
        out.push(element_to_body_item(c)?);
    }
    Ok(out)
}

fn element_to_body_item(node: Node<'_, '_>) -> AppResult<BodyItem> {
    let tag = node.tag_name().name();
    let name = node.attribute("ItemName").unwrap_or("").to_string();
    let ty = match tag {
        "Boolean" => "BOOLEAN".to_string(),
        other => other.to_ascii_uppercase(),
    };
    if ty == "L" {
        let kids: Vec<_> = node.children().filter(|n| n.is_element()).collect();
        let mut children = Vec::with_capacity(kids.len());
        for k in kids {
            children.push(element_to_body_item(k)?);
        }
        return Ok(BodyItem {
            id: Uuid::new_v4().to_string(),
            item_type: "L".into(),
            name,
            value: String::new(),
            children,
        });
    }
    let raw = node.text().unwrap_or("").trim();
    let value = if ty == "A" || ty == "J" {
        decode_ascii_text(node.text().unwrap_or(""))
    } else {
        raw.to_string()
    };
    Ok(BodyItem {
        id: Uuid::new_v4().to_string(),
        item_type: ty,
        name,
        value,
        children: vec![],
    })
}

fn decode_ascii_text(raw: &str) -> String {
    let t = raw.trim();
    // SMD sometimes wraps ASCII in quotes: `"            "`
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        // Keep internal spaces for fixed-width fields; only strip outer whitespace once.
        raw.to_string()
            .trim_start_matches('\n')
            .trim_end_matches('\n')
            .to_string()
    }
}

/// Format Secs2 as a compact SML body (no SxFy header).
pub fn secs2_to_sml_body(item: &Secs2) -> String {
    if item.is_empty_item() {
        return String::new();
    }
    format_secs2(item)
}

/// Format body tree as SML (for display / legacy field).
pub fn body_tree_to_sml(items: &[BodyItem]) -> String {
    match super::body_tree::body_tree_to_secs2(items) {
        Ok(s) => secs2_to_sml_body(&s),
        Err(_) => String::new(),
    }
}

fn format_secs2(item: &Secs2) -> String {
    use secs4rs::secs2::Secs2::*;
    match item {
        Empty => String::new(),
        List(v) if v.is_empty() => "<L>".into(),
        List(v) => {
            let inner: Vec<String> = v.iter().map(format_secs2).collect();
            format!("<L {}>", inner.join(" "))
        }
        Ascii(s) => format!("<A \"{}\">", s.replace('"', "\\\"")),
        Binary(b) => {
            let hex: Vec<String> = b.iter().map(|x| format!("0x{x:02X}")).collect();
            format!("<B {}>", hex.join(" "))
        }
        Boolean(v) => {
            let parts: Vec<&str> = v.iter().map(|b| if *b { "true" } else { "false" }).collect();
            format!("<BOOLEAN {}>", parts.join(" "))
        }
        Uint1(v) => format!("<U1 {}>", join_nums(v)),
        Uint2(v) => format!("<U2 {}>", join_nums(v)),
        Uint4(v) => format!("<U4 {}>", join_nums(v)),
        Uint8(v) => format!("<U8 {}>", join_nums(v)),
        Int1(v) => format!("<I1 {}>", join_nums(v)),
        Int2(v) => format!("<I2 {}>", join_nums(v)),
        Int4(v) => format!("<I4 {}>", join_nums(v)),
        Int8(v) => format!("<I8 {}>", join_nums(v)),
        Float4(v) => format!("<F4 {}>", join_nums(v)),
        Float8(v) => format!("<F8 {}>", join_nums(v)),
        Jis8(s) => format!("<J \"{}\">", String::from_utf8_lossy(s).replace('"', "\\\"")),
        Unicode(s) => format!("<A \"{}\">", String::from_utf8_lossy(s).replace('"', "\\\"")),
    }
}

fn join_nums<T: std::fmt::Display>(v: &[T]) -> String {
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}
