//! Focused, descriptively named tests; shared fixtures via helpers
//!
//! Docs and examples: `rules/test-structure/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "test-structure",
    area: Area::Tests,
    title: "Focused, descriptively named tests; shared fixtures via helpers",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
