//! New docs are linked from SUMMARY.md; ADRs have complete metadata; links are permalinks
//!
//! Docs and examples: `rules/docs-index-and-adr/`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::helpers::docs_deps::is_generated;
use crate::rule::{Area, Context, Edit, Findings, Fix, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "docs-index-and-adr",
    area: Area::Docs,
    title: "New docs are linked from SUMMARY.md; ADRs have complete metadata; links are permalinks",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const SUMMARY: &str = "docs/SUMMARY.md";

/// `](target)` in Markdown.
static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]\(([^)\s]+)\)").expect("valid regex"));

/// A GitHub link to a line of a file on a branch: `…/blob/main/x.rs#L12`.
static BRANCH_LINE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://github\.com/[\w.-]+/[\w.-]+/blob/([^/\s]+)/[^\s)#>\]]*#L\d+")
        .expect("valid regex")
});

fn check(cx: &Context, out: &mut Findings) {
    if let Some(summary) = cx.ws.file(SUMMARY) {
        unpublished_pages(cx, summary, out);
    }
    adr_metadata(cx, out);
    branch_line_links(cx, out);
}

/// Pages mdbook never publishes, and SUMMARY entries for missing pages.
///
/// An unlisted page gets no fix: its entry belongs in SUMMARY.md, and a fix
/// only edits the file its finding is in.
fn unpublished_pages(cx: &Context, summary: &TextFile, out: &mut Findings) {
    let mut listed = BTreeSet::new();
    let mut dead = Vec::new();
    for (line, text) in summary.lines() {
        for link in MD_LINK.captures_iter(text) {
            let target = link[1].split('#').next().unwrap_or("");
            if !target.ends_with(".md") || target.contains("://") {
                continue; // generated rustdoc pages, external links
            }
            let path = format!("docs/{}", target.trim_start_matches("./"));
            if cx.ws.file(&path).is_none() {
                dead.push((line, text, target.to_string()));
            }
            listed.insert(path);
        }
    }
    for (line, text, target) in dead {
        let message = format!("`{target}` does not exist: fix or remove this entry");
        // A page of that name that moved, when there is exactly one.
        let name = target.rsplit('/').next().unwrap_or(&target);
        let moved: Vec<&str> = cx
            .ws
            .files_with_ext(&["md"])
            .map(|f| f.path.as_str())
            .filter(|p| p.starts_with("docs/") && p.rsplit('/').next() == Some(name))
            .collect();
        let old = format!("]({target}");
        match moved.as_slice() {
            [page] if !listed.contains(*page) && text.matches(&old).count() == 1 => {
                let new = format!("]({}", page.trim_start_matches("docs/"));
                out.add_fix(
                    SUMMARY,
                    line,
                    message,
                    Fix::replace_line(line, text.replacen(&old, &new, 1)),
                );
            }
            _ => out.add(SUMMARY, line, message),
        }
    }
    for file in cx.ws.files_with_ext(&["md"]) {
        let p = file.path.as_str();
        if !p.starts_with("docs/")
            || p == SUMMARY
            || p.starts_with("docs/shared/")
            || p.starts_with("docs/book/")
            || file.file_name() == "template.md"
            || listed.contains(p)
        {
            continue;
        }
        let page = p.trim_start_matches("docs/");
        out.add(
            p,
            1,
            format!("not linked from docs/SUMMARY.md, so mdbook never publishes it: add `[…]({page})` there"),
        );
    }
}

/// ADRs need `* Status:` and `* Date:` (and `* Deciders:` while proposed),
/// and a number of their own.
fn adr_metadata(cx: &Context, out: &mut Findings) {
    let mut numbers: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for file in cx.ws.files_with_ext(&["md"]) {
        let Some(name) = file.path.strip_prefix("docs/adr/") else {
            continue;
        };
        let number: String = name.chars().take_while(char::is_ascii_digit).collect();
        if number.len() < 4 || name.contains('/') {
            continue;
        }
        numbers.entry(number).or_default().push(&file.path);
        let field = |key: &str| {
            file.lines().find_map(|(_, l)| {
                let l = l.trim_start().trim_start_matches(['*', '-']).trim_start();
                l.strip_prefix(key)
                    .and_then(|rest| rest.strip_prefix(':'))
                    .map(|v| v.trim().to_lowercase())
            })
        };
        let status = field("Status");
        let mut missing = Vec::new();
        if status.is_none() {
            missing.push("Status");
        }
        if status.as_deref().is_some_and(|s| s.starts_with("proposed"))
            && field("Deciders").is_none()
        {
            missing.push("Deciders");
        }
        if field("Date").is_none() {
            missing.push("Date");
        }
        if !missing.is_empty() {
            let named: Vec<String> = missing.iter().map(|k| format!("`* {k}:`")).collect();
            let message = format!(
                "ADR without {}: add the line(s) below the title (see docs/adr/template.md)",
                named.join(" or ")
            );
            match adr_fields_fix(file, &missing) {
                Some(fix) => out.add_fix(&file.path, 1, message, fix),
                None => out.add(&file.path, 1, message),
            }
        }
    }
    for (number, paths) in numbers {
        if paths.len() < 2 {
            continue;
        }
        for path in &paths {
            let others: Vec<&str> = paths.iter().copied().filter(|p| p != path).collect();
            out.add(
                *path,
                1,
                format!(
                    "ADR number {number} is also used by {}: renumber one of them",
                    others.join(", ")
                ),
            );
        }
    }
}

/// The metadata lines of docs/adr/template.md, with its placeholders.
const ADR_FIELDS: &[(&str, &str)] = &[
    (
        "Status",
        "* Status: [proposed | rejected | accepted | deprecated | superseded by ADR-NNNN]",
    ),
    (
        "Deciders",
        "* Deciders: [list everyone involved in the decision]",
    ),
    (
        "Date",
        "* Date: [YYYY-MM-DD when the decision was last updated]",
    ),
];

/// The line of an ADR's `* key:` metadata.
fn field_line(file: &TextFile, key: &str) -> Option<usize> {
    file.lines().find_map(|(n, l)| {
        let l = l.trim_start().trim_start_matches(['*', '-']).trim_start();
        l.strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
            .then_some(n)
    })
}

/// Add the `missing` metadata lines, with the template's placeholders, in
/// the template's order: before the next field there is, else after the
/// previous one, else below the title.
fn adr_fields_fix(file: &TextFile, missing: &[&str]) -> Option<Fix> {
    let lines: Vec<&str> = file.text.lines().collect();
    let blank = |n: usize| lines.get(n - 1).is_some_and(|l| l.trim().is_empty());
    let mut inserts: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (i, (key, placeholder)) in ADR_FIELDS.iter().enumerate() {
        if !missing.contains(key) {
            continue;
        }
        let next = ADR_FIELDS[i + 1..]
            .iter()
            .find_map(|(k, _)| field_line(file, k));
        let previous = ADR_FIELDS[..i]
            .iter()
            .rev()
            .find_map(|(k, _)| field_line(file, k));
        let at = match (next, previous) {
            (Some(next), _) => next,
            (None, Some(previous)) => {
                // After the field and its indented sub-list.
                let mut end = previous;
                while lines
                    .get(end)
                    .is_some_and(|l| l.starts_with(char::is_whitespace) && !l.trim().is_empty())
                {
                    end += 1;
                }
                end + 1
            }
            (None, None) => {
                let title = 1 + lines.iter().position(|l| l.starts_with("# "))?;
                let at = if blank(title + 1) {
                    title + 2
                } else {
                    title + 1
                };
                let entry = inserts.entry(at).or_default();
                if !blank(title + 1) {
                    entry.insert(0, String::new());
                }
                at
            }
        };
        inserts.entry(at).or_default().push(placeholder.to_string());
    }
    // Below the title, the block ends with a blank line before the text.
    let fields: Vec<usize> = ADR_FIELDS
        .iter()
        .filter_map(|(k, _)| field_line(file, k))
        .collect();
    if fields.is_empty() {
        for (at, lines_in) in inserts.iter_mut() {
            if !blank(*at) && *at <= lines.len() {
                lines_in.push(String::new());
            }
        }
    }
    Some(Fix {
        edits: inserts
            .into_iter()
            .map(|(line, insert)| Edit {
                line,
                remove: 0,
                insert,
            })
            .collect(),
    })
}

/// `blob/<branch>/…#L12` links: the line moves as the branch does.
fn branch_line_links(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files() {
        if is_generated(&file.path) || file.extension() == "json" {
            continue;
        }
        for (line, text) in file.lines() {
            for link in BRANCH_LINE_LINK.captures_iter(text) {
                let reference = &link[1];
                if is_pinned(reference) {
                    continue;
                }
                out.add(
                    &file.path,
                    line,
                    format!("links a line of `{reference}`, which drifts as the branch moves: use a commit permalink (press `y` on GitHub)"),
                );
            }
        }
    }
}

/// A commit SHA or a version tag.
fn is_pinned(reference: &str) -> bool {
    let sha = reference.len() >= 7 && reference.chars().all(|c| c.is_ascii_hexdigit());
    let tag = reference
        .trim_start_matches('v')
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit());
    sha || tag
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::docs_deps::files_from_diff;
    use crate::rule::Finding;
    use crate::testing::check;

    const SUMMARY: (&str, &str) = (
        "docs/SUMMARY.md",
        "- [ADRs](adr/README.md)\n  - [ADR-0007](adr/0007-limit-visits.md)\n- [Rustdocs](rust-docs/index.html)\n",
    );
    const ADR_7: (&str, &str) = (
        "docs/adr/0007-limit-visits.md",
        "# Limit visits\n\n* Status: accepted\n* Date: 2023-01-06\n",
    );
    const README: (&str, &str) = ("docs/adr/README.md", "# ADRs\n");

    fn run(diff: &str, summary: Option<(&str, &str)>) -> Vec<Finding> {
        let files = files_from_diff(diff);
        let mut all: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        all.extend([ADR_7, README]);
        all.extend(summary);
        check(&RULE, &all)
    }

    #[test]
    fn flags_dont() {
        let found = run(
            include_str!("../../rules/docs-index-and-adr/dont.diff"),
            Some(SUMMARY),
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
        assert!(messages.iter().any(|m| m.contains("not linked")));
        assert!(messages.iter().any(|m| m.contains("`* Deciders:`")));
        assert!(messages.iter().any(|m| m.contains("drifts")));
    }

    /// `(line, remove, insert)` of each edit of a finding's fix.
    fn edits(f: &Finding) -> Vec<(usize, usize, Vec<String>)> {
        let fix = f.fix.as_ref().expect("a fix");
        fix.edits
            .iter()
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    #[test]
    fn fixes_dont() {
        let found = run(
            include_str!("../../rules/docs-index-and-adr/dont.diff"),
            Some(SUMMARY),
        );
        let adr = found
            .iter()
            .find(|f| f.message.contains("`* Deciders:`"))
            .unwrap();
        // Between Status and Date, as in the template and do.diff.
        assert_eq!(
            edits(adr),
            vec![(
                4,
                0,
                vec!["* Deciders: [list everyone involved in the decision]".to_string()]
            )]
        );
        // The SUMMARY entry goes in another file, and links need a commit.
        assert!(found
            .iter()
            .filter(|f| !std::ptr::eq(*f, adr))
            .all(|f| f.fix.is_none()));
    }

    #[test]
    fn fix_adds_all_metadata_below_the_title_and_relinks_moved_pages() {
        let found = check(
            &RULE,
            &[
                (
                    "docs/SUMMARY.md",
                    "- [ADR-0001](adr/0001-a.md)\n- [Moved](old/moved.md)\n",
                ),
                ("docs/adr/0001-a.md", "# Use A\n\n## Context\n"),
                ("docs/howtos/moved.md", "# Moved\n"),
            ],
        );
        // SUMMARY.md, the ADR, and the moved page (not linked yet).
        assert_eq!(found.len(), 3, "{found:#?}");
        assert_eq!(
            edits(&found[0]),
            vec![(2, 1, vec!["- [Moved](howtos/moved.md)".to_string()])]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(
                3,
                0,
                vec![
                    "* Status: [proposed | rejected | accepted | deprecated | superseded by ADR-NNNN]".to_string(),
                    "* Date: [YYYY-MM-DD when the decision was last updated]".to_string(),
                    String::new(),
                ]
            )]
        );
        assert!(found[2].fix.is_none());
    }

    #[test]
    fn accepts_do() {
        let found = run(include_str!("../../rules/docs-index-and-adr/do.diff"), None);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn dead_summary_entries_and_duplicate_numbers() {
        let found = check(
            &RULE,
            &[
                (
                    "docs/SUMMARY.md",
                    "- [Gone](howtos/gone.md)\n- [A](adr/0001-a.md)\n- [B](adr/0001-b.md)\n",
                ),
                ("docs/adr/0001-a.md", "* Status: accepted\n* Date: 2021\n"),
                ("docs/adr/0001-b.md", "* Status: accepted\n* Date: 2022\n"),
            ],
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
        assert!(messages.iter().any(|m| m.contains("does not exist")));
    }

    #[test]
    fn whole_file_and_permalink_links_are_fine() {
        let text = "// https://github.com/mozilla/application-services/blob/main/Cargo.toml\n// https://github.com/mozilla/glean/blob/v60.1.0/x.rs#L3\n// https://github.com/a/b/blob/3f2a9c1/x.rs#L3-L9\n";
        assert!(check(&RULE, &[("components/x/src/lib.rs", text)]).is_empty());
    }
}
