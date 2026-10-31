//! Detect binding API changes and require a ⚠️ Breaking Changes entry
//!
//! Docs and examples: `rules/uniffi-api-break-gate/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-api-break-gate",
    area: Area::Uniffi,
    title: "Detect binding API changes and require a ⚠️ Breaking Changes entry",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
