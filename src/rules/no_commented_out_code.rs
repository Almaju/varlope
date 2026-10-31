//! Delete commented-out code
//!
//! Docs and examples: `rules/no-commented-out-code/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-commented-out-code",
    area: Area::DeadCode,
    title: "Delete commented-out code",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
