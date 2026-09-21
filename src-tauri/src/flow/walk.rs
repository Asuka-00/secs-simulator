//! Single-token graph walker (no I/O).

use std::collections::HashMap;

use crate::error::{AppError, AppResult};

use super::eval::{eval_branch, NodeResult};
use super::model::{
    BranchData, DelayData, Flow, SendData, WaitData, MAX_WALK_STEPS,
};

pub fn walk(
    flow: &Flow,
    start: Option<NodeResult>,
    mut send: impl FnMut(&SendData) -> AppResult<Option<NodeResult>>,
    mut wait: impl FnMut(&WaitData) -> AppResult<NodeResult>,
    mut delay: impl FnMut(u64),
    stopped: impl Fn() -> bool,
    mut progress: impl FnMut(&str, &str),
) -> AppResult<()> {
    let trigger = flow
        .trigger_node()
        .ok_or_else(|| AppError::Message("flow has no trigger".into()))?;
    let mut ctx: HashMap<String, NodeResult> = HashMap::new();
    let mut last_id: Option<String> = None;
    if let Some(r) = start {
        last_id = Some(trigger.id.clone());
        ctx.insert(trigger.id.clone(), r);
    }
    progress(&trigger.id, "ok");

    let mut current = flow
        .outgoing(&trigger.id, None)
        .map(|e| e.target.clone());
    let mut steps = 0u32;

    while let Some(id) = current {
        if stopped() {
            return Err(AppError::Message("flow stopped".into()));
        }
        steps += 1;
        if steps > MAX_WALK_STEPS {
            // ponytail: 128-step cap; raise if retry loops need more
            return Err(AppError::Message(format!(
                "flow exceeded {MAX_WALK_STEPS} steps"
            )));
        }
        let node = flow
            .node(&id)
            .ok_or_else(|| AppError::Message(format!("missing node {id}")))?;
        progress(&id, "running");

        match node.node_type.as_str() {
            "trigger" => {
                progress(&id, "ok");
                current = flow.outgoing(&id, None).map(|e| e.target.clone());
            }
            "send" => {
                let data: SendData = node.parse()?;
                match send(&data)? {
                    Some(r) => {
                        last_id = Some(id.clone());
                        ctx.insert(id.clone(), r);
                    }
                    None => {}
                }
                progress(&id, "ok");
                current = flow.outgoing(&id, None).map(|e| e.target.clone());
            }
            "delay" => {
                let data: DelayData = node.parse().unwrap_or(DelayData { ms: 0 });
                delay(data.ms);
                progress(&id, "ok");
                current = flow.outgoing(&id, None).map(|e| e.target.clone());
            }
            "wait" => {
                let data: WaitData = node.parse()?;
                let r = wait(&data)?;
                last_id = Some(id.clone());
                ctx.insert(id.clone(), r);
                progress(&id, "ok");
                current = flow.outgoing(&id, None).map(|e| e.target.clone());
            }
            "branch" => {
                let data: BranchData = node.parse()?;
                let ok = eval_branch(&data, last_id.as_deref(), &ctx);
                let handle = if ok { "then" } else { "else" };
                progress(&id, if ok { "then" } else { "else" });
                current = flow.outgoing(&id, Some(handle)).map(|e| e.target.clone());
            }
            other => {
                return Err(AppError::Message(format!("unknown node type {other}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::eval::first_ack_value;
    use crate::flow::model::{Flow, FlowEdge, FlowNode, FlowPos};
    use secs4rs::secs2::Secs2;

    fn node(id: &str, ty: &str, data: serde_json::Value) -> FlowNode {
        FlowNode {
            id: id.into(),
            node_type: ty.into(),
            position: FlowPos::default(),
            data,
        }
    }

    fn edge(id: &str, source: &str, target: &str, handle: Option<&str>) -> FlowEdge {
        FlowEdge {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            source_handle: handle.map(|s| s.into()),
            target_handle: None,
        }
    }

    fn commack(v: u8) -> NodeResult {
        NodeResult {
            stream: 1,
            function: 14,
            body: Secs2::binary(vec![v]).unwrap(),
            sml: String::new(),
        }
    }

    fn online_flow() -> Flow {
        Flow {
            id: "f".into(),
            name: "online".into(),
            enabled: true,
            nodes: vec![
                node("t1", "trigger", serde_json::json!({"kind":"manual"})),
                node(
                    "s13",
                    "send",
                    serde_json::json!({"stream":1,"function":13}),
                ),
                node(
                    "br",
                    "branch",
                    serde_json::json!({
                        "combinator":"and",
                        "clauses":[{"field":"ack","op":"eq","value":"0"}]
                    }),
                ),
                node(
                    "s17",
                    "send",
                    serde_json::json!({"stream":1,"function":17}),
                ),
            ],
            edges: vec![
                edge("e1", "t1", "s13", None),
                edge("e2", "s13", "br", None),
                edge("e3", "br", "s17", Some("then")),
            ],
        }
    }

    #[test]
    fn walk_branch_then_sends_s1f17() {
        let flow = online_flow();
        let mut sent = Vec::new();
        walk(
            &flow,
            None,
            |s| {
                sent.push(s.function.unwrap());
                Ok(Some(commack(0)))
            },
            |_| unreachable!(),
            |_| {},
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(sent, vec![13, 17]);
    }

    #[test]
    fn walk_branch_else_skips_s1f17() {
        let flow = online_flow();
        let mut sent = Vec::new();
        walk(
            &flow,
            None,
            |s| {
                sent.push(s.function.unwrap());
                Ok(Some(commack(1)))
            },
            |_| unreachable!(),
            |_| {},
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(sent, vec![13]);
    }

    #[test]
    fn walk_step_cap() {
        let flow = Flow {
            id: "loop".into(),
            name: "loop".into(),
            enabled: true,
            nodes: vec![
                node("t1", "trigger", serde_json::json!({"kind":"manual"})),
                node("d1", "delay", serde_json::json!({"ms":0})),
            ],
            edges: vec![
                edge("e1", "t1", "d1", None),
                edge("e2", "d1", "d1", None),
            ],
        };
        let err = walk(
            &flow,
            None,
            |_| Ok(None),
            |_| unreachable!(),
            |_| {},
            || false,
            |_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("128"), "{err}");
    }

    #[test]
    fn ack_helper() {
        assert_eq!(first_ack_value(&Secs2::binary(vec![0]).unwrap()).as_deref(), Some("0"));
    }
}
