//! No URLs, titles, usernames or record payloads in logs, errors or reports
//!
//! Docs and examples: `rules/no-pii-in-logs/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-pii-in-logs",
    area: Area::Privacy,
    title: "No URLs, titles, usernames or record payloads in logs, errors or reports",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
