//! Well-formed doc comments: `///` for items, `//!` for modules, fenced examples
//!
//! Docs and examples: `rules/doc-comment-format/`.

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "doc-comment-format",
    area: Area::Docs,
    title: "Well-formed doc comments: `///` for items, `//!` for modules, fenced examples",
    kind: Kind::Code,
    clippy: &[
        "clippy::empty_line_after_doc_comments",
        "clippy::doc_lazy_continuation",
    ],
    in_tests: false,
    check,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Blank,
    /// `///`
    Doc,
    /// `//!`
    InnerDoc,
    /// `//`, `////`
    Plain,
    /// `#[...]`
    Attr,
    Code,
}

fn classify(text: &str) -> Line {
    let t = text.trim_start();
    if t.is_empty() {
        Line::Blank
    } else if t.starts_with("////") {
        Line::Plain
    } else if t.starts_with("///") {
        Line::Doc
    } else if t.starts_with("//!") {
        Line::InnerDoc
    } else if t.starts_with("//") {
        Line::Plain
    } else if t.starts_with("#[") || t.starts_with("#![") {
        Line::Attr
    } else {
        Line::Code
    }
}

/// Comments that are notes for maintainers, not documentation.
fn is_marker(text: &str) -> bool {
    let body = text.trim_start().trim_start_matches('/').trim_start();
    let upper = body.to_uppercase();
    [
        "TODO", "FIXME", "XXX", "SAFETY", "NOTE", "HACK", "LINT", "VARLOPE:", "CLIPPY", "RUSTFMT",
        "NB",
    ]
    .iter()
    .any(|m| upper.starts_with(m))
        || body.starts_with('-')
        || body.starts_with('=')
        || body.is_empty()
}

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files_with_ext(&["rs", "udl"]) {
        if is_out_of_scope(&file.path) {
            continue;
        }
        let lines: Vec<&str> = file.text.lines().collect();
        let kinds: Vec<Line> = lines.iter().map(|l| classify(l)).collect();
        dropped_doc_lines(file, &lines, &kinds, out);
        if file.extension() == "rs" {
            missing_space(file, &lines, &kinds, out);
        }
    }
}

/// A `//` line inside a `///` block (or between it and its item) is silently
/// left out of the docs, and of the Kotlin/Swift docs generated from them.
/// Flagged when it sits between two doc lines, or when it ends a doc block
/// in a way that reads as part of it: in UDL always, in Rust when it
/// continues the doc's sentence (starts lowercase).
fn dropped_doc_lines(file: &TextFile, lines: &[&str], kinds: &[Line], out: &mut Findings) {
    let udl = file.extension() == "udl";
    let mut i = 0;
    while i < kinds.len() {
        let doc = match kinds[i] {
            Line::Doc | Line::InnerDoc => kinds[i],
            _ => {
                i += 1;
                continue;
            }
        };
        // The run of doc and plain comment lines that follows.
        let mut j = i + 1;
        while j < kinds.len() && (kinds[j] == doc || kinds[j] == Line::Plain) {
            j += 1;
        }
        let mut k = i + 1;
        while k < j {
            if kinds[k] != Line::Plain {
                k += 1;
                continue;
            }
            let start = k;
            while k < j && kinds[k] == Line::Plain {
                k += 1;
            }
            let body = plain_body(lines[start]);
            if is_marker(lines[start]) || looks_like_code(body) {
                continue;
            }
            let sandwiched = k < j;
            let next = kinds.get(k).copied().unwrap_or(Line::Blank);
            let before_item = doc == Line::Doc
                && (next == Line::Code || next == Line::Attr && !explains_attribute(&lines[k..]));
            let continues = body.chars().next().is_some_and(char::is_lowercase);
            if sandwiched || before_item && (udl || continues) {
                let what = if doc == Line::Doc { "///" } else { "//!" };
                out.add(
                    &file.path,
                    start + 1,
                    format!("`//` inside a `{what}` doc comment drops this line from the docs (and the generated Kotlin/Swift docs): use `{what}`"),
                );
            }
        }
        i = j.max(i + 1);
    }
}

/// The text of a `//` comment line.
fn plain_body(line: &str) -> &str {
    line.trim_start().trim_start_matches('/').trim()
}

/// Commented-out code rather than prose: `//pub urls: Urls,`.
fn looks_like_code(body: &str) -> bool {
    body.ends_with([',', ';', '{', '}']) || body.contains("::") && !body.contains(' ')
}

/// A comment above `#[allow(..)]` or `#[cfg(..)]` (anywhere in the attribute
/// run below it) usually explains that attribute.
fn explains_attribute(rest: &[&str]) -> bool {
    rest.iter()
        .take_while(|l| classify(l) == Line::Attr)
        .any(|l| {
            let t = l.trim_start();
            [
                "#[allow",
                "#[expect",
                "#[cfg",
                "#[deprecated",
                "#[warn",
                "#[deny",
                "#[ignore",
                "#[serde",
                "#[rustfmt",
                "#[inline",
                "#[must_use",
            ]
            .iter()
            .any(|a| t.starts_with(a))
        })
}

/// `///Same` renders, but reviewers ask for `/// Same` every time.
fn missing_space(file: &TextFile, lines: &[&str], kinds: &[Line], out: &mut Findings) {
    for (i, (line, kind)) in lines.iter().zip(kinds).enumerate() {
        let marker = match kind {
            Line::Doc => "///",
            Line::InnerDoc => "//!",
            _ => continue,
        };
        let rest = &line.trim_start()[3..];
        if rest.chars().next().is_some_and(char::is_alphanumeric) {
            out.add(
                &file.path,
                i + 1,
                format!(
                    "no space after `{marker}`: write `{marker} {}`",
                    rest.chars().take(12).collect::<String>()
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/autofill/src/db/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/doc-comment-format/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/doc-comment-format/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn notes_and_attribute_comments_are_fine() {
        let text = "/// Pops the hash.\n// TODO(bug 1234567): re-enable caching.\n#[allow(dead_code)]\nfn a() {}\n\n/// Docs.\n// needed until the next uniffi release.\n#[derive(Debug)]\n#[allow(unused)]\nstruct B;\n\n// A note for maintainers.\n\n/// Docs.\nfn c() {}\n\n/// Top-level API\n// Impl block exported via UniFFI.\n#[uniffi::export]\nimpl C {}\n\n/// The URLs.\n//pub urls: Urls,\npub urls: Vec<String>,\n";
        assert!(check(&RULE, &[(PATH, text)]).is_empty());
    }

    #[test]
    fn plain_lines_between_doc_lines() {
        let text = "//! Module docs.\n// More module docs.\n//! The end.\n\n/// First.\n// Second.\n/// Third.\nfn a() {}\n";
        let found = check(&RULE, &[(PATH, text)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 6], "{found:#?}");
    }

    #[test]
    fn udl_and_missing_spaces() {
        let udl = "interface Store {\n    /// Gets a record.\n    // Throws when the id is unknown.\n    Record get(string id);\n};\n";
        let rs = "///Same as `get`.\nfn a() {}\n//// Not a doc comment.\nfn b() {}\n";
        let found = check(&RULE, &[("components/x/src/x.udl", udl), (PATH, rs)]);
        let lines: Vec<(String, usize)> = found.iter().map(|f| (f.path.clone(), f.line)).collect();
        assert_eq!(lines.len(), 2, "{lines:#?}");
    }
}
