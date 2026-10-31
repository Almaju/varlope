//! No `!!`/force unwraps/force casts in Kotlin & Swift wrappers (incl. FML-generated code)
//!
//! Docs and examples: `rules/no-force-unwrap-wrappers/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-force-unwrap-wrappers",
    area: Area::Mobile,
    title:
        "No `!!`/force unwraps/force casts in Kotlin & Swift wrappers (incl. FML-generated code)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
