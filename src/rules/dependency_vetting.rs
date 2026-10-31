//! New/bumped crates must be vetted and compatible with mozilla-central
//!
//! Docs and examples: `rules/dependency-vetting/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "dependency-vetting",
    area: Area::Deps,
    title: "New/bumped crates must be vetted and compatible with mozilla-central",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
