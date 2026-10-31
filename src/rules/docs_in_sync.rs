//! Doc comments reference things that exist (intra-doc links, stale names)
//!
//! Docs and examples: `rules/docs-in-sync/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "docs-in-sync",
    area: Area::Docs,
    title: "Doc comments reference things that exist (intra-doc links, stale names)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
