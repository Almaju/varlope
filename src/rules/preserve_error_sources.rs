//! Keep error sources typed (`#[from]`), not stringified or mapped to an unrelated variant
//!
//! Docs and examples: `rules/preserve-error-sources/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "preserve-error-sources",
    area: Area::Errors,
    title:
        "Keep error sources typed (`#[from]`), not stringified or mapped to an unrelated variant",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
