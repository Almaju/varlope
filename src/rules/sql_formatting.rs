//! Format SQL embedded in Rust strings consistently
//!
//! Docs and examples: `rules/sql-formatting/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "sql-formatting",
    area: Area::Sql,
    title: "Format SQL embedded in Rust strings consistently",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
