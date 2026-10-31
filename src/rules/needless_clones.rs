//! Avoid needless clones and allocations; write `Arc::clone(&x)`
//!
//! Docs and examples: `rules/needless-clones/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "needless-clones",
    area: Area::Idioms,
    title: "Avoid needless clones and allocations; write `Arc::clone(&x)`",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::redundant_clone",
        "clippy::implicit_clone",
        "clippy::str_to_string",
        "clippy::clone_on_ref_ptr",
        "clippy::inefficient_to_string",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
