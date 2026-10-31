//! No panicking indexing/slicing or unchecked arithmetic on external sizes and timestamps
//!
//! Docs and examples: `rules/no-panicking-index-or-arith/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-panicking-index-or-arith",
    area: Area::Errors,
    title: "No panicking indexing/slicing or unchecked arithmetic on external sizes and timestamps",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::indexing_slicing",
        "clippy::cast_possible_truncation",
        "clippy::cast_sign_loss",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
