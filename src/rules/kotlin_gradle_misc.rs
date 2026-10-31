//! Kotlin/Gradle hygiene: injectable dispatchers, KDoc params, lazy task APIs, no lint baselines
//!
//! Docs and examples: `rules/kotlin-gradle-misc/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "kotlin-gradle-misc",
    area: Area::Mobile,
    title: "Kotlin/Gradle hygiene: injectable dispatchers, KDoc params, lazy task APIs, no lint baselines",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
