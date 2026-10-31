//! Never panic in lifts, custom-type conversions or exported constructors
//!
//! Docs and examples: `rules/uniffi-no-panic-across-ffi/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-no-panic-across-ffi",
    area: Area::Uniffi,
    title: "Never panic in lifts, custom-type conversions or exported constructors",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
