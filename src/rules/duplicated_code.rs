//! Don't copy-paste logic; reuse existing helpers
//!
//! Docs and examples: `rules/duplicated-code/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "duplicated-code",
    area: Area::Idioms,
    title: "Don't copy-paste logic; reuse existing helpers",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
