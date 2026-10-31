//! Don't export test-only helpers over FFI
//!
//! Docs and examples: `rules/uniffi-no-test-only-exports/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-no-test-only-exports",
    area: Area::Uniffi,
    title: "Don't export test-only helpers over FFI",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
