//! Well-formed doc comments: `///` for items, `//!` for modules, fenced examples
//!
//! Docs and examples: `rules/doc-comment-format/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "doc-comment-format",
    area: Area::Docs,
    title: "Well-formed doc comments: `///` for items, `//!` for modules, fenced examples",
    kind: Kind::Code,
    clippy: &[
        "clippy::empty_line_after_doc_comments",
        "clippy::doc_lazy_continuation",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
