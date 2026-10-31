//! TODO/FIXME/XXX must reference a bug
//!
//! Docs and examples: `rules/todo-needs-bug/`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::diff::{FileChange, Status};
use crate::helpers::docs_deps::{comment_lines, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "todo-needs-bug",
    area: Area::Docs,
    title: "TODO/FIXME/XXX must reference a bug",
    kind: Kind::Code,
    clippy: &["clippy::todo"],
    in_tests: true,
    check,
};

static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(TODO|FIXME|XXX)\b").expect("valid regex"));

/// A tracking reference: `bug 1234567`, a Bugzilla or GitHub issue URL, `#1234`,
/// a Jira key (`AC-206`) or a bare Bugzilla number.
static BUG_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \bbug[\s\#:_-]*\d{4,}
        | bugzil\.?la
        | bugzilla\.mozilla\.org
        | github\.com/[\w.-]+/[\w.-]+/(issues|pull)/\d+
        | \#\d{3,}
        | (?-i:\b[A-Z][A-Z0-9]+-\d+\b)
        | atlassian\.net/browse/
        | \b\d{6,8}\b",
    )
    .expect("valid regex")
});

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else {
        // The code as it is: every line of every file counts.
        for file in cx.ws.files() {
            // Test fixtures (frozen schemas, recorded data) are not edited.
            if is_out_of_scope(&file.path) || crate::workspace::is_test_path(&file.path) {
                continue;
            }
            let whole = FileChange {
                path: file.path.clone(),
                old_path: None,
                status: Status::Modified,
                added: file.lines().map(|(n, l)| (n, l.to_string())).collect(),
                removed: Vec::new(),
                old_text: None,
                new_text: Some(file.text.clone()),
            };
            report(&whole, out);
        }
        return;
    };
    for change in &diff.files {
        if change.status == Status::Deleted || is_out_of_scope(&change.path) {
            continue;
        }
        report(change, out);
    }
}

fn report(change: &FileChange, out: &mut Findings) {
    for (line, marker) in untracked_markers(change) {
        out.add(
            &change.path,
            line,
            format!("`{marker}` without a bug reference: file a bug and write `{marker}(bug NNNNNNN): …`"),
        );
    }
}

/// Added lines whose comment holds a TODO marker with no reference on it or
/// on the rest of the same comment block.
fn untracked_markers(change: &FileChange) -> Vec<(usize, String)> {
    // The comments of the whole new file when known, else of each added line.
    let comments: BTreeMap<usize, String> = match &change.new_text {
        Some(text) => comment_lines(&change.path, text),
        None => change
            .added
            .iter()
            .filter_map(|(line, text)| {
                comment_lines(&change.path, text)
                    .remove(&1)
                    .map(|c| (*line, c))
            })
            .collect(),
    };
    let mut out = Vec::new();
    for (line, _) in &change.added {
        let Some(comment) = comments.get(line) else {
            continue;
        };
        let Some(marker) = MARKER.find(comment) else {
            continue;
        };
        if is_placeholder(comment, marker.start()) {
            continue;
        }
        // The reference may sit on the next lines of the same comment.
        let tracked = (*line..line + 4)
            .map_while(|l| comments.get(&l))
            .any(|c| BUG_REF.is_match(c));
        if !tracked {
            out.push((*line, marker.as_str().to_string()));
        }
    }
    out
}

/// Not a marker: `XXX` as a placeholder (`XXX-XXXX`, `0xXXX`, `vXXX.0`), or
/// the words named in prose about markers (`TODO/FIXME`).
fn is_placeholder(comment: &str, at: usize) -> bool {
    let before = comment[..at].chars().next_back();
    let word = &comment[at..];
    let len = word
        .find(|c: char| !c.is_ascii_uppercase())
        .unwrap_or(word.len());
    let after = word[len..].chars().next();
    if before == Some('/') || after == Some('/') || before == Some('`') {
        return true;
    }
    word.starts_with("XXX")
        && (before.is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '-')
            || after.is_some_and(|c| c == '-' || c == '.'))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check_diff, check_unified};

    const PATH: &str = "components/places/src/storage.rs";

    fn added(path: &str, text: &str) -> Vec<crate::rule::Finding> {
        check_diff(&RULE, &[(path, "")], &[(path, text)])
    }

    #[test]
    fn flags_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/todo-needs-bug/dont.diff"),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 41);
    }

    #[test]
    fn accepts_do() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/todo-needs-bug/do.diff"),
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn only_added_lines_count() {
        let old = "// TODO: old debt\nfn a() {}\n";
        let new = "// TODO: old debt\nfn a() {}\nfn b() {}\n";
        assert!(check_diff(&RULE, &[(PATH, old)], &[(PATH, new)]).is_empty());
    }

    #[test]
    fn ignores_strings_identifiers_and_placeholders() {
        let text = "let s = \"TODO: not a comment\";\nstruct TodoItem;\n// Format: XXX-XXXX\n// todo lowercase is prose\n// Finds TODO/FIXME comments.\n// A `TODO` marker\n";
        assert!(added(PATH, text).is_empty());
    }

    #[test]
    fn accepts_references_on_the_next_comment_line() {
        let text = "// TODO: switch to the new API once\n// https://bugzilla.mozilla.org/show_bug.cgi?id=1234567 lands.\nfn a() {}\n// FIXME(#7612): drop this\n// XXX AC-206 follow-up\n";
        assert!(added(PATH, text).is_empty());
    }

    #[test]
    fn other_languages() {
        let diff = "--- a/automation/x.py\n+++ b/automation/x.py\n+x = 1  # TODO: retry\n+y = \"# TODO in a string\"\n--- a/docs/a.md\n+++ b/docs/a.md\n+<!-- FIXME: fill in -->\n+The TODO list is prose.\n";
        let found = check_unified(&RULE, &[], diff);
        assert_eq!(found.len(), 2, "{found:#?}");
    }
}
