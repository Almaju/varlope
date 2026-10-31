//! Don't silently discard errors (`let _ =`, `.ok()`, `if let Ok`, `_ => None`)
//!
//! Docs and examples: `rules/dont-swallow-errors/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "dont-swallow-errors",
    area: Area::Errors,
    title: "Don't silently discard errors (`let _ =`, `.ok()`, `if let Ok`, `_ => None`)",
    kind: Kind::Code,
    clippy: &["clippy::let_underscore_must_use", "clippy::map_err_ignore"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
