//! New crate manifests: license, description, `exclude` platform dirs, no deps on `examples/`
//!
//! Docs and examples: `rules/crate-manifest/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "crate-manifest",
    area: Area::Deps,
    title:
        "New crate manifests: license, description, `exclude` platform dirs, no deps on `examples/`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
