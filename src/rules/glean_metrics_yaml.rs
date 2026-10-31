//! Validate metrics.yaml: bugs, data reviews, notification emails, expiry
//!
//! Docs and examples: `rules/glean-metrics-yaml/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "glean-metrics-yaml",
    area: Area::NimbusGlean,
    title: "Validate metrics.yaml: bugs, data reviews, notification emails, expiry",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
