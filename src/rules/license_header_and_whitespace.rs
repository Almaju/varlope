//! Every source file starts with the MPL-2.0 header and ends with a newline
//!
//! Docs and examples: `rules/license-header-and-whitespace/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "license-header-and-whitespace",
    area: Area::Docs,
    title: "Every source file starts with the MPL-2.0 header and ends with a newline",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
