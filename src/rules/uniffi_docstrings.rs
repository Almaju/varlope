//! Every exported item has a `///` docstring (it is what Kotlin/Swift consumers see)
//!
//! Docs and examples: `rules/uniffi-docstrings/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-docstrings",
    area: Area::Uniffi,
    title: "Every exported item has a `///` docstring (it is what Kotlin/Swift consumers see)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
