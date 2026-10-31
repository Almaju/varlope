//! Don't hand-edit generated files or commit unrelated churn
//!
//! Docs and examples: `rules/generated-files-untouched/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "generated-files-untouched",
    area: Area::Deps,
    title: "Don't hand-edit generated files or commit unrelated churn",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
