//! Keep public error enums small and actionable; one reported catch-all with a reason
//!
//! Docs and examples: `rules/uniffi-public-errors-actionable/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-public-errors-actionable",
    area: Area::Uniffi,
    title: "Keep public error enums small and actionable; one reported catch-all with a reason",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
