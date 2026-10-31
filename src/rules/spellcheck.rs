//! Spell-check identifiers, comments, docs, UDL and CHANGELOG
//!
//! Docs and examples: `rules/spellcheck/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "spellcheck",
    area: Area::Docs,
    title: "Spell-check identifiers, comments, docs, UDL and CHANGELOG",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
