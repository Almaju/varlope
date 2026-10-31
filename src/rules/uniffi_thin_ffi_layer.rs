//! Keep the FFI layer thin: `Arc`, builders and serde stay at the boundary
//!
//! Docs and examples: `rules/uniffi-thin-ffi-layer/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-thin-ffi-layer",
    area: Area::Uniffi,
    title: "Keep the FFI layer thin: `Arc`, builders and serde stay at the boundary",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
