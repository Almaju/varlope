//! Every `unsafe` block has a `// SAFETY:` comment; NSS allocations use RAII
//!
//! Docs and examples: `rules/unsafe-and-nss/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "unsafe-and-nss",
    area: Area::Idioms,
    title: "Every `unsafe` block has a `// SAFETY:` comment; NSS allocations use RAII",
    kind: Kind::Code,
    clippy: &["clippy::undocumented_unsafe_blocks"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
