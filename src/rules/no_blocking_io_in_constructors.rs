//! No disk/network I/O in constructors of exported objects
//!
//! Docs and examples: `rules/no-blocking-io-in-constructors/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-blocking-io-in-constructors",
    area: Area::Concurrency,
    title: "No disk/network I/O in constructors of exported objects",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
