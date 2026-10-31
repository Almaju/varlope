//! Build URLs with `url::Url`, never by string concat; no hard-coded endpoints
//!
//! Docs and examples: `rules/url-hygiene/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "url-hygiene",
    area: Area::Api,
    title: "Build URLs with `url::Url`, never by string concat; no hard-coded endpoints",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
