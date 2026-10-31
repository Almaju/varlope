//! Every transaction bound to a variable is committed (drop = silent rollback)
//!
//! Docs and examples: `rules/transaction-must-commit/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "transaction-must-commit",
    area: Area::Sql,
    title: "Every transaction bound to a variable is committed (drop = silent rollback)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
