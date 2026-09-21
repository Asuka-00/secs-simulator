//! Flow graph DTOs (Vue Flow JSON compatible).

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const MAX_WALK_STEPS: u32 = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlowPos {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

impl Default for FlowPos {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub position: FlowPos,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub nodes: Vec<FlowNode>,
    #[serde(default)]
    pub edges: Vec<FlowEdge>,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: false,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerData {
    #[serde(default = "default_manual")]
    pub kind: String,
    #[serde(default)]
    pub stream: Option<i32>,
    #[serde(default)]
    pub function: Option<i32>,
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

fn default_manual() -> String {
    "manual".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendData {
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub stream: Option<i32>,
    #[serde(default)]
    pub function: Option<i32>,
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayData {
    #[serde(default)]
    pub ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitData {
    #[serde(default)]
    pub stream: i32,
    #[serde(default)]
    pub function: i32,
    #[serde(default = "default_wait_timeout")]
    pub timeout_ms: u64,
}

fn default_wait_timeout() -> u64 {
    45_000
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchClause {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default = "default_field")]
    pub field: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_op")]
    pub op: String,
    #[serde(default)]
    pub value: String,
}

fn default_field() -> String {
    "ack".into()
}
fn default_op() -> String {
    "eq".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchData {
    #[serde(default = "default_and")]
    pub combinator: String,
    #[serde(default)]
    pub clauses: Vec<BranchClause>,
}

fn default_and() -> String {
    "and".into()
}

impl FlowNode {
    pub fn parse<T: for<'de> Deserialize<'de>>(&self) -> AppResult<T> {
        serde_json::from_value(self.data.clone())
            .map_err(|e| AppError::Message(format!("node {}: {e}", self.id)))
    }
}

impl Flow {
    pub fn node(&self, id: &str) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn trigger_node(&self) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| n.node_type == "trigger")
    }

    pub fn trigger_data(&self) -> Option<TriggerData> {
        self.trigger_node()?.parse().ok()
    }

    pub fn matches_inbound(&self, stream: i32, function: i32) -> bool {
        let Some(t) = self.trigger_data() else {
            return false;
        };
        t.kind == "onInbound"
            && t.stream == Some(stream)
            && t.function == Some(function)
    }

    pub fn outgoing<'a>(&'a self, source: &str, handle: Option<&str>) -> Option<&'a FlowEdge> {
        let mut edges: Vec<&'a FlowEdge> = self
            .edges
            .iter()
            .filter(|e| e.source == source)
            .collect();
        if let Some(h) = handle {
            edges.retain(|e| e.source_handle.as_deref() == Some(h));
        } else {
            edges.retain(|e| {
                e.source_handle
                    .as_deref()
                    .map(|s| s.is_empty() || s == "source")
                    .unwrap_or(true)
            });
        }
        edges.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_vue_flow_shape() {
        let raw = r#"{
          "id": "f1",
          "name": "online",
          "enabled": true,
          "nodes": [
            {"id":"t1","type":"trigger","position":{"x":10,"y":20},"data":{"kind":"onSelected"}}
          ],
          "edges": [
            {"id":"e1","source":"t1","target":"s1"}
          ]
        }"#;
        let f: Flow = serde_json::from_str(raw).unwrap();
        assert_eq!(f.trigger_data().unwrap().kind, "onSelected");
        assert_eq!(f.outgoing("t1", None).unwrap().target, "s1");
    }
}
