//! Expose typed values over FFI (enums, Url, timestamps), not strings, magic ints or raw JSON
//!
//! Docs and examples: `rules/uniffi-typed-values/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-typed-values",
    area: Area::Uniffi,
    title:
        "Expose typed values over FFI (enums, Url, timestamps), not strings, magic ints or raw JSON",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
