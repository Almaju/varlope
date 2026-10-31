//! Remove unused pub items, error variants and columns that rustc can't see
//!
//! Docs and examples: `rules/remove-dead-pub-code/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "remove-dead-pub-code",
    area: Area::DeadCode,
    title: "Remove unused pub items, error variants and columns that rustc can't see",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
