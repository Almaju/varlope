//! No per-item queries/requests in loops; long loops check for interruption
//!
//! Docs and examples: `rules/no-per-item-queries/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-per-item-queries",
    area: Area::Sql,
    title: "No per-item queries/requests in loops; long loops check for interruption",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
