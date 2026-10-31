//! Persisted enum discriminants and hashes are stable
//!
//! Docs and examples: `rules/persisted-values-stable/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "persisted-values-stable",
    area: Area::Sql,
    title: "Persisted enum discriminants and hashes are stable",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_types"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
