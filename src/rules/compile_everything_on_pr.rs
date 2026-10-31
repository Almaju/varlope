//! Every PR compiles all targets that a change can break (wrappers, examples, tests, taskgraph)
//!
//! Docs and examples: `rules/compile-everything-on-pr/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "compile-everything-on-pr",
    area: Area::Deps,
    title: "Every PR compiles all targets that a change can break (wrappers, examples, tests, taskgraph)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
