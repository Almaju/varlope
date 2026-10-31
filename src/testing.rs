//! Helpers for rule tests.

use crate::diff::Diff;
use crate::rule::{Finding, Rule};
use crate::workspace::Workspace;

/// Run one rule on in-memory files.
pub fn check(rule: &'static Rule, files: &[(&str, &str)]) -> Vec<Finding> {
    let ws = Workspace::from_files(files);
    crate::run::run(&ws, None, &[rule])
}

/// Run one rule on a change between two sets of in-memory files. The
/// workspace is the "after" side.
pub fn check_diff(rule: &'static Rule, old: &[(&str, &str)], new: &[(&str, &str)]) -> Vec<Finding> {
    let ws = Workspace::from_files(new);
    let diff = Diff::from_texts(old, new);
    crate::run::run(&ws, Some(&diff), &[rule])
}

/// Run one rule on a `git diff -U0`-style text (only hunks, no whole files).
pub fn check_unified(rule: &'static Rule, files: &[(&str, &str)], unified: &str) -> Vec<Finding> {
    let ws = Workspace::from_files(files);
    let diff = Diff::from_unified(unified);
    crate::run::run(&ws, Some(&diff), &[rule])
}
