//! Callback interfaces return `Result`, are released on shutdown, and user-waiting calls are async
//!
//! Docs and examples: `rules/uniffi-callback-contract/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-callback-contract",
    area: Area::Uniffi,
    title: "Callback interfaces return `Result`, are released on shutdown, and user-waiting calls are async",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
