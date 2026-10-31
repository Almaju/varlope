//! Printing findings, and the report file the site is built from.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::replay::Commit;
use crate::rule::{Finding, Rule};

pub fn print_text(findings: &[Finding]) {
    for f in findings {
        let at = match f.line {
            0 => f.path.clone(),
            line => format!("{}:{}:{}", f.path, line, f.column),
        };
        let commit = f
            .commit
            .as_deref()
            .map(|c| format!(" [{}]", &c[..c.len().min(12)]))
            .unwrap_or_default();
        println!("{at}: {}{commit}: {}", f.rule, f.message);
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in findings {
        *counts.entry(f.rule).or_default() += 1;
    }
    if !findings.is_empty() {
        eprintln!(
            "\n{} finding(s) from {} rule(s)",
            findings.len(),
            counts.len()
        );
    }
}

/// What `varlope report` writes: everything the site needs about one run.
#[derive(Serialize)]
pub struct Report<'a> {
    /// The repository and commit that was linted.
    pub repo: String,
    pub sha: String,
    pub date: String,
    pub clippy: bool,
    pub rules: Vec<RuleInfo>,
    pub findings: &'a [Finding],
    pub commits: &'a [Commit],
}

#[derive(Serialize)]
pub struct RuleInfo {
    pub id: &'static str,
    pub area: crate::rule::Area,
    pub area_title: &'static str,
    pub title: &'static str,
    pub kind: crate::rule::Kind,
    pub kind_label: &'static str,
    pub clippy: &'static [&'static str],
}

impl RuleInfo {
    pub fn of(rule: &'static Rule) -> Self {
        Self {
            id: rule.id,
            area: rule.area,
            area_title: rule.area.title(),
            title: rule.title,
            kind: rule.kind,
            kind_label: rule.kind.label(),
            clippy: rule.clippy,
        }
    }
}
