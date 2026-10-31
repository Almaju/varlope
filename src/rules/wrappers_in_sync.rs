//! Hand-written Kotlin/Swift wrappers stay in sync with the Rust API and don't redefine UDL types
//!
//! Docs and examples: `rules/wrappers-in-sync/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "wrappers-in-sync",
    area: Area::Mobile,
    title: "Hand-written Kotlin/Swift wrappers stay in sync with the Rust API and don't redefine UDL types",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
