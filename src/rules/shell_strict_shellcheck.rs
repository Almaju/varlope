//! Shell scripts use `set -euo pipefail` and stay portable (shellcheck alone misses both)
//!
//! Docs and examples: `rules/shell-strict-shellcheck/`.

use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "shell-strict-shellcheck",
    area: Area::Scripts,
    title: "Shell scripts use `set -euo pipefail` and stay portable (shellcheck alone misses both)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let _ = (cx, out); // TODO: not implemented yet
}
