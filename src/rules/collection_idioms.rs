//! Use `collect`/`extend`/entry API; compile regexes once
//!
//! Docs and examples: `rules/collection-idioms/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "collection-idioms",
    area: Area::Idioms,
    title: "Use `collect`/`extend`/entry API; compile regexes once",
    kind: Kind::Clippy,
    clippy: &["clippy::needless_collect", "clippy::explicit_iter_loop"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
