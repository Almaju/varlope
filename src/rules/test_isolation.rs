//! Tests are isolated: no wall clock, shared global state, fixed DB paths or prod endpoints
//!
//! Docs and examples: `rules/test-isolation/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "test-isolation",
    area: Area::Tests,
    title:
        "Tests are isolated: no wall clock, shared global state, fixed DB paths or prod endpoints",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
