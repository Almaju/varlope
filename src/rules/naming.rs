//! Descriptive identifiers: no cryptic abbreviations, follow Rust API naming
//!
//! Docs and examples: `rules/naming/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "naming",
    area: Area::Api,
    title: "Descriptive identifiers: no cryptic abbreviations, follow Rust API naming",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_names"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
