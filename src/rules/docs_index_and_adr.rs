//! New docs are linked from SUMMARY.md; ADRs have complete metadata; links are permalinks
//!
//! Docs and examples: `rules/docs-index-and-adr/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "docs-index-and-adr",
    area: Area::Docs,
    title: "New docs are linked from SUMMARY.md; ADRs have complete metadata; links are permalinks",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
