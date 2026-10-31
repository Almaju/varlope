//! No wildcard arms on domain enums; destructure fully in merge/hash/eq impls
//!
//! Docs and examples: `rules/exhaustive-matches/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "exhaustive-matches",
    area: Area::Idioms,
    title: "No wildcard arms on domain enums; destructure fully in merge/hash/eq impls",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::wildcard_enum_match_arm",
        "clippy::match_wildcard_for_single_variants",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
