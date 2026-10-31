//! Sync payloads round-trip unknown fields and tolerate nulls
//!
//! Docs and examples: `rules/sync-payload-compat/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "sync-payload-compat",
    area: Area::Remote,
    title: "Sync payloads round-trip unknown fields and tolerate nulls",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
