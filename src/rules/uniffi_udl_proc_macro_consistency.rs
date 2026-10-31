//! UDL and proc-macro declarations agree; new code uses proc-macros
//!
//! Docs and examples: `rules/uniffi-udl-proc-macro-consistency/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-udl-proc-macro-consistency",
    area: Area::Uniffi,
    title: "UDL and proc-macro declarations agree; new code uses proc-macros",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
