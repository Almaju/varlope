//! No `println!`/`eprintln!`/`dbg!` (or `printStackTrace`/`print`) in shipped code
//!
//! Docs and examples: `rules/no-debug-leftovers/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-debug-leftovers",
    area: Area::DeadCode,
    title: "No `println!`/`eprintln!`/`dbg!` (or `printStackTrace`/`print`) in shipped code",
    kind: Kind::Code,
    clippy: &[
        "clippy::dbg_macro",
        "clippy::print_stdout",
        "clippy::print_stderr",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
