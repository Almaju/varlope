//! Typed durations, sizes and timestamps; don't substitute `now` for bad data
//!
//! Docs and examples: `rules/typed-time-and-units/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "typed-time-and-units",
    area: Area::Api,
    title: "Typed durations, sizes and timestamps; don't substitute `now` for bad data",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
