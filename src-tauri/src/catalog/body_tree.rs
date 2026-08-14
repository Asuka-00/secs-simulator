//! Structured SECS-II body tree (for UI tree editor + SMD ItemName).
//! 结构化 SECS-II 正文树（供 UI 树编辑器 + SMD ItemName）。

use secs4rs::secs2::Secs2;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// One node in the editable body tree (mirrors GWGEM DataItem tree).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BodyItem {
    pub id: String,
    /// `L` | `A` | `B` | `BOOLEAN` | `U1`…`U8` | `I1`…`I8` | `F4` | `F8`
    #[serde(rename = "type")]
    pub item_type: String,
    /// SMD `ItemName` (optional label).
    #[serde(default)]
    pub name: String,
    /// Leaf value(s), space-separated for multi-value numerics / binary.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub children: Vec<BodyItem>,
}

impl BodyItem {
    pub fn new_list(name: impl Into<String>, children: Vec<BodyItem>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            item_type: "L".into(),
            name: name.into(),
            value: String::new(),
            children,
        }
    }

    pub fn new_leaf(ty: impl Into<String>, name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            item_type: ty.into(),
            name: name.into(),
            value: value.into(),
            children: vec![],
        }
    }
}

/// Convert body tree → Secs2 for wire send.
pub fn body_tree_to_secs2(items: &[BodyItem]) -> AppResult<Secs2> {
    if items.is_empty() {
        return Ok(Secs2::empty());
    }
    if items.len() == 1 {
        return item_to_secs2(&items[0]);
    }
    let mut kids = Vec::with_capacity(items.len());
    for it in items {
        kids.push(item_to_secs2(it)?);
    }
    Secs2::list(kids).map_err(|e| AppError::Message(format!("body list: {e}")))
}

fn item_to_secs2(it: &BodyItem) -> AppResult<Secs2> {
    let ty = it.item_type.to_ascii_uppercase();
    match ty.as_str() {
        "L" => {
            if it.children.is_empty() {
                return Ok(Secs2::list_empty());
            }
            let mut kids = Vec::with_capacity(it.children.len());
            for c in &it.children {
                kids.push(item_to_secs2(c)?);
            }
            Secs2::list(kids).map_err(|e| AppError::Message(format!("L: {e}")))
        }
        "A" | "J" => Secs2::ascii(it.value.clone()).map_err(|e| AppError::Message(format!("A: {e}"))),
        "B" => {
            let bytes = parse_bytes(&it.value)?;
            Secs2::binary(bytes).map_err(|e| AppError::Message(format!("B: {e}")))
        }
        "BOOLEAN" | "BOOL" => {
            let vals = parse_bools(&it.value)?;
            Secs2::bool_values(vals).map_err(|e| AppError::Message(format!("BOOLEAN: {e}")))
        }
        "U1" => Secs2::uint1(parse_u64s(&it.value)?.into_iter().map(|v| v as u8))
            .map_err(|e| AppError::Message(format!("U1: {e}"))),
        "U2" => Secs2::uint2(parse_u64s(&it.value)?.into_iter().map(|v| v as u16))
            .map_err(|e| AppError::Message(format!("U2: {e}"))),
        "U4" => Secs2::uint4(parse_u64s(&it.value)?.into_iter().map(|v| v as u32))
            .map_err(|e| AppError::Message(format!("U4: {e}"))),
        "U8" => Secs2::uint8(parse_u64s(&it.value)?)
            .map_err(|e| AppError::Message(format!("U8: {e}"))),
        "I1" => Secs2::int1(parse_i64s(&it.value)?.into_iter().map(|v| v as i8))
            .map_err(|e| AppError::Message(format!("I1: {e}"))),
        "I2" => Secs2::int2(parse_i64s(&it.value)?.into_iter().map(|v| v as i16))
            .map_err(|e| AppError::Message(format!("I2: {e}"))),
        "I4" => Secs2::int4(parse_i64s(&it.value)?.into_iter().map(|v| v as i32))
            .map_err(|e| AppError::Message(format!("I4: {e}"))),
        "I8" => Secs2::int8(parse_i64s(&it.value)?)
            .map_err(|e| AppError::Message(format!("I8: {e}"))),
        "F4" => Secs2::float4(parse_f64s(&it.value)?.into_iter().map(|v| v as f32))
            .map_err(|e| AppError::Message(format!("F4: {e}"))),
        "F8" => Secs2::float8(parse_f64s(&it.value)?)
            .map_err(|e| AppError::Message(format!("F8: {e}"))),
        other => Err(AppError::Message(format!("unsupported body type: {other}"))),
    }
}

/// Secs2 → body tree (no ItemNames).
pub fn secs2_to_body_tree(item: &Secs2) -> Vec<BodyItem> {
    if item.is_empty_item() {
        return vec![];
    }
    vec![secs2_to_item(item)]
}

fn secs2_to_item(item: &Secs2) -> BodyItem {
    use secs4rs::secs2::Secs2::*;
    match item {
        Empty => BodyItem::new_list("", vec![]),
        List(v) => BodyItem::new_list(
            "",
            v.iter().map(secs2_to_item).collect(),
        ),
        Ascii(s) => BodyItem::new_leaf("A", "", s.clone()),
        Binary(b) => {
            let v = b
                .iter()
                .map(|x| format!("0x{x:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            BodyItem::new_leaf("B", "", v)
        }
        Boolean(v) => {
            let s = v
                .iter()
                .map(|b| if *b { "true" } else { "false" })
                .collect::<Vec<_>>()
                .join(" ");
            BodyItem::new_leaf("BOOLEAN", "", s)
        }
        Uint1(v) => BodyItem::new_leaf("U1", "", join_nums(v)),
        Uint2(v) => BodyItem::new_leaf("U2", "", join_nums(v)),
        Uint4(v) => BodyItem::new_leaf("U4", "", join_nums(v)),
        Uint8(v) => BodyItem::new_leaf("U8", "", join_nums(v)),
        Int1(v) => BodyItem::new_leaf("I1", "", join_nums(v)),
        Int2(v) => BodyItem::new_leaf("I2", "", join_nums(v)),
        Int4(v) => BodyItem::new_leaf("I4", "", join_nums(v)),
        Int8(v) => BodyItem::new_leaf("I8", "", join_nums(v)),
        Float4(v) => BodyItem::new_leaf("F4", "", join_nums(v)),
        Float8(v) => BodyItem::new_leaf("F8", "", join_nums(v)),
        Jis8(s) => BodyItem::new_leaf("A", "", String::from_utf8_lossy(s).into_owned()),
        Unicode(s) => BodyItem::new_leaf("A", "", String::from_utf8_lossy(s).into_owned()),
    }
}

fn join_nums<T: std::fmt::Display>(v: &[T]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
}

fn parse_bytes(raw: &str) -> AppResult<Vec<u8>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(vec![]);
    }
    t.split_whitespace()
        .map(|p| {
            if let Some(hex) = p.strip_prefix("0x").or_else(|| p.strip_prefix("0X")) {
                u8::from_str_radix(hex, 16)
                    .map_err(|_| AppError::Message(format!("bad binary: {p}")))
            } else {
                p.parse::<u8>()
                    .map_err(|_| AppError::Message(format!("bad binary: {p}")))
            }
        })
        .collect()
}

fn parse_bools(raw: &str) -> AppResult<Vec<bool>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(vec![]);
    }
    Ok(t.split_whitespace()
        .map(|p| matches!(p.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "t"))
        .collect())
}

fn parse_u64s(raw: &str) -> AppResult<Vec<u64>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(vec![0]);
    }
    t.split_whitespace()
        .map(|p| {
            if let Some(hex) = p.strip_prefix("0x").or_else(|| p.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16)
                    .map_err(|_| AppError::Message(format!("bad uint: {p}")))
            } else {
                p.parse::<u64>()
                    .map_err(|_| AppError::Message(format!("bad uint: {p}")))
            }
        })
        .collect()
}

fn parse_i64s(raw: &str) -> AppResult<Vec<i64>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(vec![0]);
    }
    t.split_whitespace()
        .map(|p| {
            p.parse::<i64>()
                .map_err(|_| AppError::Message(format!("bad int: {p}")))
        })
        .collect()
}

fn parse_f64s(raw: &str) -> AppResult<Vec<f64>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(vec![0.0]);
    }
    t.split_whitespace()
        .map(|p| {
            p.parse::<f64>()
                .map_err(|_| AppError::Message(format!("bad float: {p}")))
        })
        .collect()
}
