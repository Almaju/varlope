//! Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`
//!
//! Docs and examples: `rules/parking-lot-locks/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "parking-lot-locks",
    area: Area::Concurrency,
    title: "Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_types"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
