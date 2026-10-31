//! Use Option/Result combinators and `let-else` instead of manual matches
//!
//! Docs and examples: `rules/option-result-combinators/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "option-result-combinators",
    area: Area::Idioms,
    title: "Use Option/Result combinators and `let-else` instead of manual matches",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::manual_let_else",
        "clippy::option_if_let_else",
        "clippy::map_unwrap_or",
        "clippy::unnecessary_wraps",
        "clippy::match_same_arms",
        "clippy::if_not_else",
        "clippy::redundant_closure_for_method_calls",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
