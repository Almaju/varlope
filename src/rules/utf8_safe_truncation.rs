//! Never truncate or slice strings at byte offsets
//!
//! Docs and examples: `rules/utf8-safe-truncation/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "utf8-safe-truncation",
    area: Area::Errors,
    title: "Never truncate or slice strings at byte offsets",
    kind: Kind::Code,
    clippy: &["clippy::string_slice", "clippy::disallowed_methods"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
