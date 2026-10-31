//! TODO/FIXME/XXX must reference a bug
//!
//! Docs and examples: `rules/todo-needs-bug/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "todo-needs-bug",
    area: Area::Docs,
    title: "TODO/FIXME/XXX must reference a bug",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
