//! Let rustfmt own imports and layout (incl. inside macros)
//!
//! Docs and examples: `rules/rustfmt-config/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "rustfmt-config",
    area: Area::Idioms,
    title: "Let rustfmt own imports and layout (incl. inside macros)",
    kind: Kind::Clippy,
    clippy: &["clippy::wildcard_imports", "unused_qualifications"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
