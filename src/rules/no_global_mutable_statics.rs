//! No `static Mutex<…>` caches for state that belongs in the DB or a struct
//!
//! Docs and examples: `rules/no-global-mutable-statics/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-global-mutable-statics",
    area: Area::Concurrency,
    title: "No `static Mutex<…>` caches for state that belongs in the DB or a struct",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
