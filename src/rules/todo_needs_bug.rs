//! TODO/FIXME/XXX must reference a bug
//!
//! Docs and examples: `rules/todo-needs-bug/`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::diff::{FileChange, Status};
use crate::helpers::docs_deps::{comment_lines, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};

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
    for (line, marker, tail) in untracked_markers(change) {
        let message = format!(
            "`{marker}` without a bug reference: file a bug and write `{marker}(bug NNNNNNN): …`"
        );
        let fixed = change
            .added
            .iter()
            .find(|(n, _)| *n == line)
            .and_then(|(_, text)| with_placeholder(text, &tail));
        match fixed {
            Some(text) => out.add_fix(&change.path, line, message, Fix::replace_line(line, text)),
            None => out.add(&change.path, line, message),
        }
    }
}

/// Added lines whose comment holds a TODO marker with no reference on it or
/// on the rest of the same comment block: `(line, marker, the comment from
/// the marker on)`.
fn untracked_markers(change: &FileChange) -> Vec<(usize, String, String)> {
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
            let tail = if comment[..marker.start()]
                .trim_matches(|c: char| c.is_whitespace() || "(>*!/-".contains(c))
                .is_empty()
            {
                comment[marker.start()..].to_string()
            } else if explicit_separator(&comment[marker.end()..]) {
                // `We are in m-c - XXX - this is temporary`.
                comment[marker.start()..].to_string()
            } else {
                String::new() // `this is TODO.`: prose, no obvious rewrite
            };
            out.push((*line, marker.as_str().to_string(), tail));
        }
    }
    out
}

/// `line` with `(bug NNNNNNN)` after the marker that starts `tail` (the
/// comment from the marker on): `// TODO: x` -> `// TODO(bug NNNNNNN): x`,
/// `# XXX - x` -> `# XXX(bug NNNNNNN): x`, `// TODO(lina): x` ->
/// `// TODO(bug NNNNNNN, lina): x`. `None` when the marker is not found.
fn with_placeholder(line: &str, tail: &str) -> Option<String> {
    if tail.is_empty() {
        return None;
    }
    // The comment is the end of the line (bar a `*/` or `-->`): the last
    // place the start of `tail` appears.
    let mut cut = tail.len().min(24);
    while !tail.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut at = line.rfind(&tail[..cut])?;
    let mut len = MARKER.find(&line[at..]).filter(|m| m.start() == 0)?.end();
    // `XXX TODO: x`, `XXX - FIXME - x`: the reference goes on the last marker.
    while let Some(rest) = separator(&line[at + len..]) {
        match MARKER.find(rest) {
            Some(next) if next.start() == 0 => {
                at = line.len() - rest.len();
                len = next.end();
            }
            _ => break,
        }
    }
    let marker = &line[at..at + len];
    let mut after = &line[at + len..];
    let mut reference = "(bug NNNNNNN)".to_string();
    if let Some(inner) = after.strip_prefix('(') {
        let close = inner.find(')')?;
        reference = format!("(bug NNNNNNN, {})", &inner[..close]);
        after = &inner[close + 1..];
    }
    let text = separator(after)?;
    let rest = if after.starts_with(':') {
        after.to_string() // as written: `TODO: x`, `**TODO:**`
    } else if text.is_empty() {
        String::new()
    } else if text.starts_with("*/") || text.starts_with("-->") {
        format!(" {text}")
    } else {
        format!(": {text}")
    };
    Some(format!("{}{marker}{reference}{rest}", &line[..at]))
}

/// What follows a marker once its separator is dropped (`: x`, ` - x`, ` x`
/// -> `x`), or `None` when the marker runs into other text (`TODO.`).
fn separator(after: &str) -> Option<&str> {
    if after.is_empty() {
        return Some(after);
    }
    if let Some(rest) = after.strip_prefix(':') {
        return Some(rest.trim_start());
    }
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let text = after.trim_start();
    if let Some(rest) = text.strip_prefix(':') {
        return Some(rest.trim_start());
    }
    let dashes = text.trim_start_matches(['-', '–']);
    if dashes.len() < text.len() && (dashes.is_empty() || dashes.starts_with(char::is_whitespace)) {
        return Some(dashes.trim_start());
    }
    Some(text)
}

/// A marker followed by `:` or ` - `, as in `… - XXX - this is temporary`.
fn explicit_separator(after: &str) -> bool {
    after.starts_with(':')
        || after.starts_with(char::is_whitespace) && after.trim_start().starts_with(['-', '–', ':'])
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

    /// The line a finding's fix writes in place of its own.
    fn fixed(f: &crate::rule::Finding) -> Option<&str> {
        let fix = f.fix.as_ref()?;
        assert_eq!(fix.edits.len(), 1, "{fix:#?}");
        let e = &fix.edits[0];
        assert_eq!(
            (e.line, e.remove, e.insert.len()),
            (f.line, 1, 1),
            "{fix:#?}"
        );
        Some(&e.insert[0])
    }

    #[test]
    fn fix_adds_a_bug_placeholder_to_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/todo-needs-bug/dont.diff"),
        );
        assert_eq!(
            fixed(&found[0]),
            Some(
                "    // TODO(bug NNNNNNN): switch to the icu4x collator. // flagged: no bug to track it"
            )
        );
    }

    #[test]
    fn fix_placeholders_in_each_marker_style() {
        let text = "// XXX - check more.\n\
            # comment-free line\n\
            // TODO(lina): Same as above.\n\
            //!   **XXX TODO:**\n\
            // XXX - FIXME - searching fails?\n\
            #[allow(clippy::x)] // FIXME\n\
            /* TODO */\n\
            // so this is TODO.\n\
            // We are in m-c - XXX - this is temporary\n";
        let found = added(PATH, text);
        let fixes: Vec<Option<&str>> = found.iter().map(fixed).collect();
        assert_eq!(
            fixes,
            vec![
                Some("// XXX(bug NNNNNNN): check more."),
                Some("// TODO(bug NNNNNNN, lina): Same as above."),
                Some("//!   **XXX TODO(bug NNNNNNN):**"),
                Some("// XXX - FIXME(bug NNNNNNN): searching fails?"),
                Some("#[allow(clippy::x)] // FIXME(bug NNNNNNN)"),
                Some("/* TODO(bug NNNNNNN) */"),
                None,
                Some("// We are in m-c - XXX(bug NNNNNNN): this is temporary"),
            ],
            "{found:#?}"
        );
        let py = added(
            "tools/a.py",
            "x = {\"a\": \"\"}  # XXX beetmover wants it\n",
        );
        assert_eq!(
            fixed(&py[0]),
            Some("x = {\"a\": \"\"}  # XXX(bug NNNNNNN): beetmover wants it")
        );
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
