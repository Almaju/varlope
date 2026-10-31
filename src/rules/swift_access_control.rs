//! Swift: explicit access control for consumer-facing API
//!
//! Docs and examples: `rules/swift-access-control/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "swift-access-control",
    area: Area::Mobile,
    title: "Swift: explicit access control for consumer-facing API",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
