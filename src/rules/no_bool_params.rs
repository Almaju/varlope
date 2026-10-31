//! Replace boolean flags/positional tuples with enums or named structs
//!
//! Docs and examples: `rules/no-bool-params/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-bool-params",
    area: Area::Api,
    title: "Replace boolean flags/positional tuples with enums or named structs",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::fn_params_excessive_bools",
        "clippy::struct_excessive_bools",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
