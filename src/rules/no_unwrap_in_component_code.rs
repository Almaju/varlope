//! No `unwrap()`/`expect()`/`panic!` in non-test component code
//!
//! Docs and examples: `rules/no-unwrap-in-component-code/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-unwrap-in-component-code",
    area: Area::Errors,
    title: "No `unwrap()`/`expect()`/`panic!` in non-test component code",
    kind: Kind::Code,
    clippy: &[
        "clippy::unwrap_used",
        "clippy::expect_used",
        "clippy::panic",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
