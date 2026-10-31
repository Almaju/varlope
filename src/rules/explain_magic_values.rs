//! Name and explain magic values and non-obvious intent
//!
//! Docs and examples: `rules/explain-magic-values/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "explain-magic-values",
    area: Area::Docs,
    title: "Name and explain magic values and non-obvious intent",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
