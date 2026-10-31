//! Optional deps and the code that uses them sit behind the same feature; check every combination
//!
//! Docs and examples: `rules/cargo-feature-hygiene/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "cargo-feature-hygiene",
    area: Area::Deps,
    title: "Optional deps and the code that uses them sit behind the same feature; check every combination",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
