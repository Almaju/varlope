//! Use the narrowest visibility (`pub(crate)`/private)
//!
//! Docs and examples: `rules/minimal-visibility/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "minimal-visibility",
    area: Area::Api,
    title: "Use the narrowest visibility (`pub(crate)`/private)",
    kind: Kind::Clippy,
    clippy: &["unreachable_pub"],
    in_tests: false,
    check,
};

fn check(_cx: &Context, _out: &mut Findings) {
    // rustc's `unreachable_pub` lint implements this rule (`varlope check --clippy`).
}
