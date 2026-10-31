//! A small curated pedantic subset (derives, unit structs, `Default` use)
//!
//! Docs and examples: `rules/pedantic-subset/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "pedantic-subset",
    area: Area::Idioms,
    title: "A small curated pedantic subset (derives, unit structs, `Default` use)",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::uninlined_format_args",
        "clippy::semicolon_if_nothing_returned",
        "clippy::default_trait_access",
        "clippy::empty_structs_with_brackets",
        "clippy::empty_enum_variants_with_brackets",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
