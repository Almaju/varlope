//! CHANGELOG edits go in the in-progress section, under a component heading, with a PR link
//!
//! Docs and examples: `rules/changelog-format/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "changelog-format",
    area: Area::Docs,
    title:
        "CHANGELOG edits go in the in-progress section, under a component heading, with a PR link",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
