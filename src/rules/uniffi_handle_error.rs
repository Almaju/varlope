//! Exported fallible functions return `ApiResult` via `#[handle_error]`; internal code uses the crate `Error`
//!
//! Docs and examples: `rules/uniffi-handle-error/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-handle-error",
    area: Area::Uniffi,
    title: "Exported fallible functions return `ApiResult` via `#[handle_error]`; internal code uses the crate `Error`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
