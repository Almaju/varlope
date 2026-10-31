//! Pin CI tools, images and actions (no `:latest`, current major of actions)
//!
//! Docs and examples: `rules/pin-ci-tools/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "pin-ci-tools",
    area: Area::Scripts,
    title: "Pin CI tools, images and actions (no `:latest`, current major of actions)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
