//! New behaviour comes with tests in the same PR
//!
//! Docs and examples: `rules/tests-with-behaviour/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "tests-with-behaviour",
    area: Area::Tests,
    title: "New behaviour comes with tests in the same PR",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
