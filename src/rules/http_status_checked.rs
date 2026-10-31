//! Check the HTTP status before using a response body; only explicit success is success
//!
//! Docs and examples: `rules/http-status-checked/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "http-status-checked",
    area: Area::Errors,
    title: "Check the HTTP status before using a response body; only explicit success is success",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
