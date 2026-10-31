//! Run rules over a workspace and keep the findings that count.

use crate::diff::Diff;
use crate::rule::{Context, Finding, Findings, Kind, Origin, Rule};
use crate::workspace::{is_test_path, Workspace};

/// Run `rules` and return their findings, minus those covered by an allow
/// comment or sitting in test code (unless the rule asks for tests).
/// Diff rules only run when there is a diff.
pub fn run(ws: &Workspace, diff: Option<&Diff>, rules: &[&'static Rule]) -> Vec<Finding> {
    run_with(ws, ws, diff, rules)
}

/// Like [`run`], but rules see `ws` while findings are filtered (test code,
/// allow comments) against `filter`: for a past commit, the files as they
/// were at that commit.
pub fn run_with(
    ws: &Workspace,
    filter: &Workspace,
    diff: Option<&Diff>,
    rules: &[&'static Rule],
) -> Vec<Finding> {
    let cx = Context { ws, diff };
    let mut all = Vec::new();
    for rule in rules {
        if rule.kind == Kind::Diff && diff.is_none() {
            continue;
        }
        let mut out = Findings::new(rule.id);
        (rule.check)(&cx, &mut out);
        all.extend(out.into_vec().into_iter().filter(|f| keep(filter, rule, f)));
    }
    sort(&mut all);
    all
}

/// The allow comments that do not work (no reason, no rule list).
pub fn allow_problems(ws: &Workspace) -> Vec<Finding> {
    let mut out = Vec::new();
    for file in ws.files() {
        let problems = match ws.rust(&file.path) {
            Some(rust) => rust.allowances.problems().to_vec(),
            None => file.allowances.problems().to_vec(),
        };
        for p in problems {
            out.push(Finding {
                rule: "allow-comment",
                path: file.path.clone(),
                line: p.line,
                column: 1,
                message: p.message,
                origin: Origin::Varlope,
                commit: None,
            });
        }
    }
    out
}

/// Whether a finding survives allow comments and test-code filtering.
pub fn keep(ws: &Workspace, rule: &Rule, f: &Finding) -> bool {
    let line = f.line.max(1);
    if let Some(rust) = ws.rust(&f.path) {
        if !rule.in_tests && f.line > 0 && rust.in_test(f.line) {
            return false;
        }
        if rust.allowances.covers(rule.id, line) {
            return false;
        }
    } else if let Some(file) = ws.file(&f.path) {
        if file.allowances.covers(rule.id, line) {
            return false;
        }
    }
    if !rule.in_tests && f.path.ends_with(".rs") && is_test_path(&f.path) {
        return false;
    }
    true
}

pub fn sort(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        (a.path.as_str(), a.line, a.column, a.rule).cmp(&(
            b.path.as_str(),
            b.line,
            b.column,
            b.rule,
        ))
    });
}
