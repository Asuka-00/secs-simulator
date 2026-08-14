//! Declarative auto-reply rule engine.

pub mod engine;

pub use engine::{
    evaluate_and_apply, new_shared_rules, parse_secs2_body, Rule, RuleAction, RuleMatch, RuleOutcome,
    RuleSet, SharedRules,
};
