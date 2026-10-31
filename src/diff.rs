//! A change under review: what a PR adds and removes, file by file.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context as _, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Debug)]
pub struct FileChange {
    /// Path after the change (before it, for a deletion).
    pub path: String,
    /// Path before a rename.
    pub old_path: Option<String>,
    pub status: Status,
    /// `(new line number, text)` of each added line.
    pub added: Vec<(usize, String)>,
    /// `(old line number, text)` of each removed line.
    pub removed: Vec<(usize, String)>,
    /// Whole file before / after the change, when known (text files only).
    pub old_text: Option<String>,
    pub new_text: Option<String>,
}

impl FileChange {
    fn empty(path: String) -> Self {
        Self {
            path,
            old_path: None,
            status: Status::Modified,
            added: Vec::new(),
            removed: Vec::new(),
            old_text: None,
            new_text: None,
        }
    }

    pub fn added_text(&self) -> impl Iterator<Item = &str> {
        self.added.iter().map(|(_, t)| t.as_str())
    }

    pub fn removed_text(&self) -> impl Iterator<Item = &str> {
        self.removed.iter().map(|(_, t)| t.as_str())
    }

    /// Whether any added or removed line matches `pred`.
    pub fn touches_line(&self, pred: impl Fn(&str) -> bool) -> bool {
        self.added_text().chain(self.removed_text()).any(pred)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Diff {
    pub base: String,
    pub head: String,
    pub files: Vec<FileChange>,
}

impl Diff {
    /// The change from `base` to `head` (a ref; `None` = the working tree).
    pub fn from_git(root: &Path, base: &str, head: Option<&str>) -> Result<Self> {
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U0", "-M", base];
        if let Some(head) = head {
            args.push(head);
        }
        let out = git(root, &args)?;
        let mut diff = Self::from_unified(&out);
        diff.base = base.to_string();
        diff.head = head.unwrap_or("working tree").to_string();
        for change in &mut diff.files {
            let old = change
                .old_path
                .clone()
                .unwrap_or_else(|| change.path.clone());
            if change.status != Status::Added {
                change.old_text = git(root, &["show", &format!("{base}:{old}")]).ok();
            }
            if change.status != Status::Deleted {
                change.new_text = match head {
                    Some(head) => git(root, &["show", &format!("{head}:{}", change.path)]).ok(),
                    None => std::fs::read_to_string(root.join(&change.path)).ok(),
                };
            }
        }
        Ok(diff)
    }

    /// Parse unified diff text (`git diff` output, or a hand-written diff
    /// with only `---`/`+++` headers). Whole-file texts are left empty.
    pub fn from_unified(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let git_style = lines.iter().any(|l| l.starts_with("diff --git "));
        let mut files: Vec<FileChange> = Vec::new();
        let (mut old_line, mut new_line) = (0usize, 0usize);
        // Inside a hunk, `+++`/`---` are content, not file headers.
        let mut in_hunk = false;
        for (i, line) in lines.iter().enumerate() {
            let starts_file = if git_style {
                line.starts_with("diff --git ")
            } else {
                line.starts_with("--- ") && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ "))
            };
            if starts_file {
                in_hunk = false;
                let path = line
                    .strip_prefix("diff --git ")
                    .and_then(|rest| rest.rsplit_once(" b/"))
                    .map(|(_, p)| p.to_string())
                    .unwrap_or_default();
                files.push(FileChange::empty(path));
                if git_style {
                    continue;
                }
            }
            let Some(file) = files.last_mut() else {
                continue;
            };
            if in_hunk {
                if let Some(added) = line.strip_prefix('+') {
                    file.added.push((new_line, added.to_string()));
                    new_line += 1;
                    continue;
                }
                if let Some(removed) = line.strip_prefix('-') {
                    file.removed.push((old_line, removed.to_string()));
                    old_line += 1;
                    continue;
                }
                if line.starts_with(' ') || line.is_empty() {
                    old_line += 1;
                    new_line += 1;
                    continue;
                }
            }
            if line.starts_with("new file mode") {
                file.status = Status::Added;
            } else if line.starts_with("deleted file mode") {
                file.status = Status::Deleted;
            } else if let Some(from) = line.strip_prefix("rename from ") {
                file.status = Status::Renamed;
                file.old_path = Some(from.to_string());
            } else if let Some(to) = line.strip_prefix("rename to ") {
                file.path = to.to_string();
            } else if let Some(minus) = line.strip_prefix("--- ") {
                if let Some(p) = minus.strip_prefix("a/") {
                    if file.path.is_empty() || file.status == Status::Deleted {
                        file.path = p.to_string();
                    }
                } else if minus.starts_with("/dev/null") {
                    file.status = Status::Added;
                }
            } else if let Some(plus) = line.strip_prefix("+++ ") {
                if let Some(p) = plus.strip_prefix("b/") {
                    file.path = p.to_string();
                } else if plus.starts_with("/dev/null") {
                    file.status = Status::Deleted;
                }
                // Hand-written diffs may skip the `@@` header.
                in_hunk = true;
                old_line = 1;
                new_line = 1;
            } else if let Some(hunk) = line.strip_prefix("@@ ") {
                in_hunk = true;
                let mut parts = hunk.split_whitespace();
                old_line = parse_start(parts.next().unwrap_or("-0"));
                new_line = parse_start(parts.next().unwrap_or("+0"));
                // A zero-length side (`-3,0`) means lines go *after* that line.
                if parts_len_zero(hunk, '-') {
                    old_line += 1;
                }
                if parts_len_zero(hunk, '+') {
                    new_line += 1;
                }
            }
        }
        Self {
            base: String::new(),
            head: String::new(),
            files,
        }
    }

    /// A change between two sets of in-memory files, for tests.
    pub fn from_texts(old: &[(&str, &str)], new: &[(&str, &str)]) -> Self {
        let old: BTreeMap<&str, &str> = old.iter().copied().collect();
        let new: BTreeMap<&str, &str> = new.iter().copied().collect();
        let mut files = Vec::new();
        let paths: std::collections::BTreeSet<&str> =
            old.keys().chain(new.keys()).copied().collect();
        for path in paths {
            let (o, n) = (old.get(path).copied(), new.get(path).copied());
            if o == n {
                continue;
            }
            let status = match (o, n) {
                (None, Some(_)) => Status::Added,
                (Some(_), None) => Status::Deleted,
                _ => Status::Modified,
            };
            let (removed, added) = line_diff(o.unwrap_or(""), n.unwrap_or(""));
            files.push(FileChange {
                path: path.to_string(),
                old_path: None,
                status,
                added,
                removed,
                old_text: o.map(str::to_string),
                new_text: n.map(str::to_string),
            });
        }
        Self {
            base: "old".into(),
            head: "new".into(),
            files,
        }
    }

    pub fn file(&self, path: &str) -> Option<&FileChange> {
        self.files.iter().find(|f| f.path == path)
    }

    /// Changed files whose path matches `pred`.
    pub fn changed<'a>(
        &'a self,
        pred: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = &'a FileChange> {
        self.files.iter().filter(move |f| pred(&f.path))
    }

    pub fn touches(&self, path: &str) -> bool {
        self.files.iter().any(|f| f.path == path)
    }
}

/// Whether the `-` or `+` range of a hunk header has length 0 (`-12,0`).
fn parts_len_zero(hunk: &str, sign: char) -> bool {
    hunk.split_whitespace()
        .find(|p| p.starts_with(sign))
        .is_some_and(|p| p.ends_with(",0"))
}

fn parse_start(range: &str) -> usize {
    range[1..]
        .split(',')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// `(line number, text)` pairs.
type Lines = Vec<(usize, String)>;

/// Removed and added lines between two texts (longest common subsequence).
pub(crate) fn line_diff(old: &str, new: &str) -> (Lines, Lines) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            added.push((j + 1, b[j].to_string()));
            j += 1;
        } else {
            removed.push((i + 1, a[i].to_string()));
            i += 1;
        }
    }
    (removed, added)
}

pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unified_diff() {
        let text = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3 +3,2 @@\n-old\n+new\n+more\ndiff --git a/b.md b/b.md\nnew file mode 100644\n--- /dev/null\n+++ b/b.md\n@@ -0,0 +1 @@\n+hello\n";
        let diff = Diff::from_unified(text);
        assert_eq!(diff.files.len(), 2);
        let a = &diff.files[0];
        assert_eq!(a.path, "src/a.rs");
        assert_eq!(a.removed, vec![(3, "old".to_string())]);
        assert_eq!(
            a.added,
            vec![(3, "new".to_string()), (4, "more".to_string())]
        );
        assert_eq!(diff.files[1].status, Status::Added);
    }

    #[test]
    fn parses_hand_written_diffs() {
        let text = "--- a/x.udl\n+++ b/x.udl\n@@ -1 +1,2 @@ interface A {\n void a();\n+void b();\n--- a/CHANGELOG.md\n+++ b/CHANGELOG.md\n ## What's New\n+- b\n";
        let diff = Diff::from_unified(text);
        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.files[0].path, "x.udl");
        assert_eq!(diff.files[0].added, vec![(2, "void b();".to_string())]);
        assert_eq!(diff.files[1].path, "CHANGELOG.md");
        assert_eq!(diff.files[1].added, vec![(2, "- b".to_string())]);
    }

    #[test]
    fn diffs_texts() {
        let diff = Diff::from_texts(
            &[("a", "x\ny\nz\n")],
            &[("a", "x\nY\nz\nw\n"), ("b", "new\n")],
        );
        let a = diff.file("a").unwrap();
        assert_eq!(a.removed, vec![(2, "y".to_string())]);
        assert_eq!(a.added, vec![(2, "Y".to_string()), (4, "w".to_string())]);
        assert_eq!(diff.file("b").unwrap().status, Status::Added);
    }
}
