//! PRAGMAs are set only in the connection initializer and documented
//!
//! Docs and examples: `rules/pragmas-centralized/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "pragmas-centralized",
    area: Area::Sql,
    title: "PRAGMAs are set only in the connection initializer and documented",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
