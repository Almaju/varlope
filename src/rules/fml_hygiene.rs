//! FML: pinned remote imports, backward-compatible manifests, clean templates and fixtures
//!
//! Docs and examples: `rules/fml-hygiene/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "fml-hygiene",
    area: Area::NimbusGlean,
    title:
        "FML: pinned remote imports, backward-compatible manifests, clean templates and fixtures",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
