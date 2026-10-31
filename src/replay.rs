//! Run the diff rules over past commits, as if each had been a PR.

use std::path::Path;

use anyhow::Result;

use crate::diff::{git, Diff};
use crate::rule::{Finding, Kind, Rule};
use crate::workspace::Workspace;

/// A commit that was replayed.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    /// The PR number, from a `(#1234)` at the end of the subject.
    pub pr: Option<u64>,
}

/// Replay the last `count` first-parent commits of HEAD through every diff
/// rule. `ws` is the current checkout, used for context only.
pub fn replay(
    root: &Path,
    ws: &Workspace,
    rules: &[&'static Rule],
    count: usize,
) -> Result<(Vec<Commit>, Vec<Finding>)> {
    let diff_rules: Vec<&'static Rule> = rules
        .iter()
        .copied()
        .filter(|r| r.kind == Kind::Diff)
        .collect();
    let log = git(
        root,
        &[
            "log",
            "--first-parent",
            "--format=%H%x09%s",
            "-n",
            &count.to_string(),
        ],
    )?;
    let mut commits = Vec::new();
    let mut findings = Vec::new();
    for line in log.lines() {
        let Some((sha, subject)) = line.split_once('\t') else {
            continue;
        };
        let Ok(diff) = Diff::from_git(root, &format!("{sha}^"), Some(sha)) else {
            continue; // the first commit of a shallow clone has no parent
        };
        for mut f in crate::run::run(ws, Some(&diff), &diff_rules) {
            f.commit = Some(sha.to_string());
            findings.push(f);
        }
        commits.push(Commit {
            sha: sha.to_string(),
            subject: subject.to_string(),
            pr: pr_number(subject),
        });
    }
    Ok((commits, findings))
}

fn pr_number(subject: &str) -> Option<u64> {
    let open = subject.rfind("(#")?;
    subject[open + 2..].trim_end_matches(')').parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn pr_numbers() {
        assert_eq!(super::pr_number("Bug 1 - Fix it (#7687)"), Some(7687));
        assert_eq!(super::pr_number("Merge branch"), None);
    }
}
