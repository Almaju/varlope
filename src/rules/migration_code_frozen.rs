//! Migrations use frozen SQL and fixtures; tests assert the `VERSION` constant
//!
//! Docs and examples: `rules/migration-code-frozen/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "migration-code-frozen",
    area: Area::Sql,
    title: "Migrations use frozen SQL and fixtures; tests assert the `VERSION` constant",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
