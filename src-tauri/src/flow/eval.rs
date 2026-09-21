//! Branch compare + SECS-II body path extract.

use secs4rs::hsms::HsmsMessage;
use secs4rs::secs2::Secs2;

use super::model::{BranchClause, BranchData};

#[derive(Debug, Clone)]
pub struct NodeResult {
    pub stream: i32,
    pub function: i32,
    pub body: Secs2,
    pub sml: String,
}

impl NodeResult {
    pub fn sx_fy(&self) -> String {
        format!("S{}F{}", self.stream, self.function)
    }

    pub fn from_hsms(msg: &HsmsMessage) -> Self {
        let body = msg.secs2().clone();
        Self {
            stream: msg.get_stream(),
            function: msg.get_function(),
            sml: compact_sml(msg.get_stream(), msg.get_function(), &body),
            body,
        }
    }
}

pub fn parse_path(path: &str) -> Vec<usize> {
    path.split('.')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// First numeric / binary / bool / ascii leaf (GEM ACK-style).
pub fn first_ack_value(item: &Secs2) -> Option<String> {
    match item {
        Secs2::List(xs) => xs.iter().find_map(first_ack_value),
        Secs2::Binary(b) if !b.is_empty() => Some(b[0].to_string()),
        Secs2::Uint1(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Uint2(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Uint4(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Uint8(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Int1(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Int2(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Int4(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Int8(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Float4(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Float8(v) if !v.is_empty() => Some(v[0].to_string()),
        Secs2::Boolean(v) if !v.is_empty() => Some(if v[0] { "1".into() } else { "0".into() }),
        Secs2::Ascii(s) => Some(s.clone()),
        Secs2::Empty => None,
        _ => None,
    }
}

pub fn item_at_path(body: &Secs2, path: &str) -> Option<Secs2> {
    body.get_item(&parse_path(path)).ok().cloned()
}

pub fn field_value(result: &NodeResult, field: &str, path: Option<&str>) -> Option<String> {
    match field {
        "sxFy" => Some(result.sx_fy()),
        "sml" => Some(result.sml.clone()),
        "bodyPath" => {
            let item = item_at_path(&result.body, path.unwrap_or(""))?;
            first_ack_value(&item)
        }
        _ => first_ack_value(&result.body),
    }
}

pub fn compare(op: &str, left: &str, right: &str) -> bool {
    let ln = left.parse::<f64>().ok();
    let rn = right.parse::<f64>().ok();
    match op {
        "ne" => {
            if let (Some(a), Some(b)) = (ln, rn) {
                a != b
            } else {
                left != right
            }
        }
        "gt" => match (ln, rn) {
            (Some(a), Some(b)) => a > b,
            _ => left > right,
        },
        "lt" => match (ln, rn) {
            (Some(a), Some(b)) => a < b,
            _ => left < right,
        },
        "contains" => left.contains(right),
        _ => {
            if let (Some(a), Some(b)) = (ln, rn) {
                a == b
            } else {
                left == right
            }
        }
    }
}

pub fn eval_clause(
    clause: &BranchClause,
    last_id: Option<&str>,
    ctx: &std::collections::HashMap<String, NodeResult>,
) -> bool {
    let id = clause
        .node_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(last_id);
    let Some(id) = id else {
        return false;
    };
    let Some(result) = ctx.get(id) else {
        return false;
    };
    let Some(left) = field_value(result, &clause.field, clause.path.as_deref()) else {
        return false;
    };
    compare(&clause.op, &left, &clause.value)
}

pub fn eval_branch(
    data: &BranchData,
    last_id: Option<&str>,
    ctx: &std::collections::HashMap<String, NodeResult>,
) -> bool {
    if data.clauses.is_empty() {
        return true;
    }
    let or_mode = data.combinator.eq_ignore_ascii_case("or");
    if or_mode {
        data.clauses.iter().any(|c| eval_clause(c, last_id, ctx))
    } else {
        data.clauses.iter().all(|c| eval_clause(c, last_id, ctx))
    }
}

pub fn compact_sml(stream: i32, function: i32, body: &Secs2) -> String {
    format!("S{stream}F{function} {}", compact_item(body))
}

fn compact_item(item: &Secs2) -> String {
    match item {
        Secs2::List(xs) => {
            let inner: Vec<String> = xs.iter().map(compact_item).collect();
            format!("<L {}>", inner.join(" "))
        }
        Secs2::Ascii(s) => format!("<A \"{}\">", s),
        Secs2::Binary(b) if !b.is_empty() => format!("<B 0x{:02X}>", b[0]),
        Secs2::Binary(_) => "<B>".into(),
        Secs2::Uint1(v) if !v.is_empty() => format!("<U1 {}>", v[0]),
        Secs2::Int1(v) if !v.is_empty() => format!("<I1 {}>", v[0]),
        Secs2::Empty => String::new(),
        other => format!("<{other:?}>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::model::BranchClause;

    fn commack(v: u8) -> NodeResult {
        NodeResult {
            stream: 1,
            function: 14,
            body: Secs2::list([
                Secs2::binary(vec![v]).unwrap(),
                Secs2::list_empty(),
            ])
            .unwrap(),
            sml: format!("S1F14 COMMACK={v}"),
        }
    }

    #[test]
    fn ack_eq_zero() {
        let r = commack(0);
        assert_eq!(first_ack_value(&r.body).as_deref(), Some("0"));
        assert!(compare("eq", "0", "0"));
        assert!(!compare("eq", "1", "0"));
    }

    #[test]
    fn body_path_nested() {
        let body = Secs2::list([
            Secs2::binary(vec![0]).unwrap(),
            Secs2::list([Secs2::ascii("SIM").unwrap()]).unwrap(),
        ])
        .unwrap();
        let item = item_at_path(&body, "1.0").unwrap();
        assert_eq!(first_ack_value(&item).as_deref(), Some("SIM"));
    }

    #[test]
    fn branch_and_or() {
        let mut ctx = std::collections::HashMap::new();
        ctx.insert("s1".into(), commack(0));
        let ok = BranchClause {
            node_id: Some("s1".into()),
            field: "ack".into(),
            path: None,
            op: "eq".into(),
            value: "0".into(),
        };
        let bad = BranchClause {
            node_id: Some("s1".into()),
            field: "ack".into(),
            path: None,
            op: "eq".into(),
            value: "1".into(),
        };
        assert!(eval_branch(
            &BranchData {
                combinator: "and".into(),
                clauses: vec![ok.clone()],
            },
            None,
            &ctx
        ));
        assert!(!eval_branch(
            &BranchData {
                combinator: "and".into(),
                clauses: vec![ok.clone(), bad.clone()],
            },
            None,
            &ctx
        ));
        assert!(eval_branch(
            &BranchData {
                combinator: "or".into(),
                clauses: vec![ok, bad],
            },
            None,
            &ctx
        ));
    }
}
