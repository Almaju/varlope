//! `build.rs` writes only to `OUT_DIR`, doesn't shell out, fails with actionable messages
//!
//! Docs and examples: `rules/build-rs-hygiene/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "build-rs-hygiene",
    area: Area::Deps,
    title: "`build.rs` writes only to `OUT_DIR`, doesn't shell out, fails with actionable messages",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
