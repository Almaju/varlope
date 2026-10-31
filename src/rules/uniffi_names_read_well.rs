//! Exported names must read well in Kotlin/Swift and say what they do
//!
//! Docs and examples: `rules/uniffi-names-read-well/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-names-read-well",
    area: Area::Uniffi,
    title: "Exported names must read well in Kotlin/Swift and say what they do",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
