//! Acquire a lock once per operation, at the layer that owns the state
//!
//! Docs and examples: `rules/lock-once-per-operation/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "lock-once-per-operation",
    area: Area::Concurrency,
    title: "Acquire a lock once per operation, at the layer that owns the state",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
