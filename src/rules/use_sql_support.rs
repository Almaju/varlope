//! Use `sql_support` helpers (`ConnExt`, cached statements, `ConnectionInitializer`) instead of raw rusqlite
//!
//! Docs and examples: `rules/use-sql-support/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "use-sql-support",
    area: Area::Sql,
    title: "Use `sql_support` helpers (`ConnExt`, cached statements, `ConnectionInitializer`) instead of raw rusqlite",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
