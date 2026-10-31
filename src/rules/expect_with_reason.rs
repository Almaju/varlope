//! No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`
//!
//! Docs and examples: `rules/expect-with-reason/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "expect-with-reason",
    area: Area::DeadCode,
    title: "No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`",
    kind: Kind::Code,
    clippy: &["clippy::allow_attributes_without_reason"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
