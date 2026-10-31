//! Tables filled from Remote Settings records carry `record_id` and are cleaned on deletion
//!
//! Docs and examples: `rules/rs-record-cleanup/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "rs-record-cleanup",
    area: Area::Sql,
    title:
        "Tables filled from Remote Settings records carry `record_id` and are cleaned on deletion",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
