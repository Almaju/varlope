//! Apply findings' suggested fixes to the checkout.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::rule::{Edit, Finding};

/// What `apply` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Findings whose fix was written.
    pub fixed: usize,
    /// Findings whose fix overlapped one already taken; run again for them.
    pub skipped: usize,
}

/// Write the fixes of `findings` to the files under `root`. A fix is taken
/// whole or not at all; one that overlaps a fix already taken in the same
/// file is skipped.
pub fn apply(root: &Path, findings: &[Finding]) -> Result<Applied> {
    let mut by_file: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in findings.iter().filter(|f| f.fix.is_some()) {
        by_file.entry(f.path.as_str()).or_default().push(f);
    }
    let mut applied = Applied::default();
    for (path, fs) in by_file {
        let full = root.join(path);
        let text = std::fs::read_to_string(&full).with_context(|| format!("reading {path}"))?;
        let edits: Vec<&[Edit]> = fs
            .iter()
            .filter_map(|f| f.fix.as_ref().map(|x| x.edits.as_slice()))
            .collect();
        let (new, taken) = apply_to_text(&text, &edits);
        applied.fixed += taken;
        applied.skipped += edits.len() - taken;
        if new != text {
            std::fs::write(&full, new).with_context(|| format!("writing {path}"))?;
        }
    }
    Ok(applied)
}

/// Apply each fix (a group of edits) that does not overlap one before it.
/// Returns the new text and how many fixes were taken.
pub fn apply_to_text(text: &str, fixes: &[&[Edit]]) -> (String, usize) {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let last = lines.len();
    let mut taken: Vec<(usize, &Edit)> = Vec::new(); // (fix order, edit)
    let mut count = 0;
    for (order, edits) in fixes.iter().enumerate() {
        let clashes = edits
            .iter()
            .any(|e| taken.iter().any(|(_, t)| overlaps(e, t)));
        if !clashes
            && edits
                .iter()
                .all(|e| e.line >= 1 && e.line + e.remove <= last + 1)
        {
            taken.extend(edits.iter().map(|e| (order, e)));
            count += 1;
        }
    }
    let touches_end = taken.iter().any(|(_, e)| e.line + e.remove > last);
    // Bottom-up, so earlier line numbers stay valid. On one line, replace
    // before inserting, and insert later fixes first so they end up after.
    taken.sort_by(|(oa, a), (ob, b)| {
        b.line
            .cmp(&a.line)
            .then((b.remove > 0).cmp(&(a.remove > 0)))
            .then(ob.cmp(oa))
    });
    for (_, e) in taken {
        let at = e.line - 1;
        lines.splice(at..at + e.remove, e.insert.iter().cloned());
    }
    let mut out = lines.join("\n");
    if !out.is_empty() && (text.ends_with('\n') || touches_end) {
        out.push('\n');
    }
    (out, count)
}

/// Whether two edits touch the same lines. An insertion only clashes with
/// a replacement it would land inside.
fn overlaps(a: &Edit, b: &Edit) -> bool {
    match (a.remove, b.remove) {
        (0, 0) => false,
        (0, _) => b.line < a.line && a.line < b.line + b.remove,
        (_, 0) => a.line < b.line && b.line < a.line + a.remove,
        _ => a.line < b.line + b.remove && b.line < a.line + a.remove,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(line: usize, remove: usize, insert: &[&str]) -> Edit {
        Edit {
            line,
            remove,
            insert: insert.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn applies_bottom_up_and_skips_overlaps() {
        let text = "a\nb\nc\n";
        let one = [edit(2, 1, &["B"])];
        let two = [edit(2, 0, &["/// doc"])];
        let three = [edit(2, 2, &["x"])]; // overlaps `one`
        let four = [edit(4, 0, &["d"])]; // appends
        let (out, n) = apply_to_text(text, &[&one, &two, &three, &four]);
        assert_eq!(out, "a\n/// doc\nB\nc\nd\n");
        assert_eq!(n, 3);
    }

    #[test]
    fn insertions_keep_their_order() {
        let (out, _) = apply_to_text("a\n", &[&[edit(1, 0, &["1"])], &[edit(1, 0, &["2"])]]);
        assert_eq!(out, "1\n2\na\n");
    }

    #[test]
    fn rewriting_the_last_line_adds_the_final_newline() {
        let (out, n) = apply_to_text("a\nb", &[&[edit(2, 1, &["b"])]]);
        assert_eq!((out.as_str(), n), ("a\nb\n", 1));
        let (out, _) = apply_to_text("a\nb", &[&[edit(1, 1, &["A"])]]);
        assert_eq!(out, "A\nb");
    }

    #[test]
    fn deletions() {
        let (out, _) = apply_to_text("a\nb\n\n\n", &[&[edit(3, 2, &[])]]);
        assert_eq!(out, "a\nb\n");
    }
}
