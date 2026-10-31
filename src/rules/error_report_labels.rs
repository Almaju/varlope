//! `report_error!` labels are component-prefixed, unique, and not on hot paths
//!
//! Docs and examples: `rules/error-report-labels/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "error-report-labels",
    area: Area::Errors,
    title: "`report_error!` labels are component-prefixed, unique, and not on hot paths",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
