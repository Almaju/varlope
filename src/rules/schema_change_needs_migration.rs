//! Any schema change bumps `VERSION`, adds an `upgrade_from` step and an upgrade test
//!
//! Docs and examples: `rules/schema-change-needs-migration/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "schema-change-needs-migration",
    area: Area::Sql,
    title: "Any schema change bumps `VERSION`, adds an `upgrade_from` step and an upgrade test",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
