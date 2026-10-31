//! Optional/new Record fields get a UniFFI default that matches Rust's `Default`; avoid `Option<Vec<_>>`
//!
//! Docs and examples: `rules/uniffi-record-defaults/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-record-defaults",
    area: Area::Uniffi,
    title: "Optional/new Record fields get a UniFFI default that matches Rust's `Default`; avoid `Option<Vec<_>>`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
