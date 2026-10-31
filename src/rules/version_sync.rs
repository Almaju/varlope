//! One UniFFI/Glean version everywhere; toolchain versions in sync; no accidental submodule moves
//!
//! Docs and examples: `rules/version-sync/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "version-sync",
    area: Area::Deps,
    title: "One UniFFI/Glean version everywhere; toolchain versions in sync; no accidental submodule moves",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
