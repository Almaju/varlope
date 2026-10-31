//! SQLite footguns: `INSERT OR REPLACE`, `COUNT(*)` for existence, `LIKE` prefix, unsafe `LOWER()`
//!
//! Docs and examples: `rules/sqlite-footguns/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "sqlite-footguns",
    area: Area::Sql,
    title: "SQLite footguns: `INSERT OR REPLACE`, `COUNT(*)` for existence, `LIKE` prefix, unsafe `LOWER()`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
