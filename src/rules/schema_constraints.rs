//! Columns declare NOT NULL/keys, FKs declare ON DELETE and are indexed, new indexes are justified
//!
//! Docs and examples: `rules/schema-constraints/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "schema-constraints",
    area: Area::Sql,
    title: "Columns declare NOT NULL/keys, FKs declare ON DELETE and are indexed, new indexes are justified",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
