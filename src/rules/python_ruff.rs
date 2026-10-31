//! Python automation is formatted and linted (ruff) and exits non-zero on failure
//!
//! Docs and examples: `rules/python-ruff/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "python-ruff",
    area: Area::Scripts,
    title: "Python automation is formatted and linted (ruff) and exits non-zero on failure",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
