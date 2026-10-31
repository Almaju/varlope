//! Don't let HashMap/HashSet order leak into results, storage or generated output
//!
//! Docs and examples: `rules/deterministic-iteration/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "deterministic-iteration",
    area: Area::Idioms,
    title: "Don't let HashMap/HashSet order leak into results, storage or generated output",
    kind: Kind::Code,
    clippy: &["clippy::iter_over_hash_type"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
