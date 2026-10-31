//! New/renamed components are registered everywhere (megazords, uniffi.toml, Swift, Gradle, CI)
//!
//! Docs and examples: `rules/component-wiring/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "component-wiring",
    area: Area::Deps,
    title: "New/renamed components are registered everywhere (megazords, uniffi.toml, Swift, Gradle, CI)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
