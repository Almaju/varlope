//! Never call foreign callbacks, do network I/O, or re-lock while holding a store lock
//!
//! Docs and examples: `rules/no-callbacks-or-io-under-lock/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-callbacks-or-io-under-lock",
    area: Area::Concurrency,
    title: "Never call foreign callbacks, do network I/O, or re-lock while holding a store lock",
    kind: Kind::Code,
    clippy: &[
        "clippy::significant_drop_tightening",
        "clippy::significant_drop_in_scrutinee",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
