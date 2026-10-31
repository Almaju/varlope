//! No unused or misplaced dependencies
//!
//! Docs and examples: `rules/no-unused-deps/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-unused-deps",
    area: Area::DeadCode,
    title: "No unused or misplaced dependencies",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
