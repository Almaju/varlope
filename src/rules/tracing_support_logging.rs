//! Log through `tracing_support`, not the `log` crate
//!
//! Docs and examples: `rules/tracing-support-logging/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "tracing-support-logging",
    area: Area::Privacy,
    title: "Log through `tracing_support`, not the `log` crate",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_macros"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
