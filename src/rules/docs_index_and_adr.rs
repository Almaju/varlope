//! New docs are linked from SUMMARY.md; ADRs have complete metadata; links are permalinks
//!
//! Docs and examples: `rules/docs-index-and-adr/`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::helpers::docs_deps::is_generated;
use crate::rule::{Area, Context, Findings, Kind, Rule};
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
fn unpublished_pages(cx: &Context, summary: &TextFile, out: &mut Findings) {
    let mut listed = BTreeSet::new();
    for (line, text) in summary.lines() {
        for link in MD_LINK.captures_iter(text) {
            let target = link[1].split('#').next().unwrap_or("");
            if !target.ends_with(".md") || target.contains("://") {
                continue; // generated rustdoc pages, external links
            }
            let path = format!("docs/{}", target.trim_start_matches("./"));
            if cx.ws.file(&path).is_none() {
                out.add(
                    SUMMARY,
                    line,
                    format!("`{target}` does not exist: fix or remove this entry"),
                );
            }
            listed.insert(path);
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
            missing.push("`* Status:`");
        }
        if status.as_deref().is_some_and(|s| s.starts_with("proposed"))
            && field("Deciders").is_none()
        {
            missing.push("`* Deciders:`");
        }
        if field("Date").is_none() {
            missing.push("`* Date:`");
        }
        if !missing.is_empty() {
            out.add(
                &file.path,
                1,
                format!(
                    "ADR without {}: add the line(s) below the title (see docs/adr/template.md)",
                    missing.join(" or ")
                ),
            );
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
