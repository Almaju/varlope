//! Tests must actually assert: no bare `matches!`, mockito mocks asserted, messages match
//!
//! Docs and examples: `rules/meaningful-assertions/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "meaningful-assertions",
    area: Area::Tests,
    title: "Tests must actually assert: no bare `matches!`, mockito mocks asserted, messages match",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
