//! Types deserialized from server/Remote Settings data tolerate absent and unknown values
//!
//! Docs and examples: `rules/tolerant-remote-deserialization/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "tolerant-remote-deserialization",
    area: Area::Remote,
    title: "Types deserialized from server/Remote Settings data tolerate absent and unknown values",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
