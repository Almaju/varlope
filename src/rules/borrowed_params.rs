//! Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`
//!
//! Docs and examples: `rules/borrowed-params/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "borrowed-params",
    area: Area::Idioms,
    title: "Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::needless_pass_by_value",
        "clippy::ref_option",
        "clippy::trivially_copy_pass_by_ref",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
