//! Every source file starts with the MPL-2.0 header and ends with a newline
//!
//! Docs and examples: `rules/license-header-and-whitespace/`.

use std::collections::BTreeSet;

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Edit, Findings, Fix, Kind, Rule};
use crate::source::{RustFile, TextFile};

pub static RULE: Rule = Rule {
    id: "license-header-and-whitespace",
    area: Area::Docs,
    title: "Every source file starts with the MPL-2.0 header and ends with a newline",
    kind: Kind::Code,
    clippy: &[],
    // mozilla-central lints test files too.
    in_tests: true,
    check,
};

/// Files that carry the MPL header.
const HEADER_EXTS: &[&str] = &["rs", "kt", "kts", "swift", "py", "sh", "udl", "gradle"];
/// Files whose whitespace is checked (data files such as JSON and `.txt` are not).
const WHITESPACE_EXTS: &[&str] = &[
    "rs", "kt", "kts", "swift", "py", "sh", "udl", "gradle", "toml", "yaml", "yml", "md", "sql",
    "xml",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files() {
        let path = file.path.as_str();
        if is_out_of_scope(path) || file.text.is_empty() {
            continue;
        }
        let ext = file.extension();
        // Code templates: a header there would be copied into generated code.
        if HEADER_EXTS.contains(&ext) && !path.contains("/templates/") && !has_header(&file.text) {
            let message = "no MPL-2.0 header: start the file with the license.template text, in this file's comment syntax";
            match header_fix(file) {
                Some(fix) => out.add_fix(path, 1, message, fix),
                None => out.add(path, 1, message),
            }
        }
        if WHITESPACE_EXTS.contains(&ext) && !path.split('/').any(|p| p == "fixtures") {
            let in_strings = cx.ws.rust(path).map(string_lines).unwrap_or_default();
            whitespace(file, &in_strings, out);
        }
    }
}

/// `mozilla.org/MPL/2.0` in the first 10 lines after any shebang.
fn has_header(text: &str) -> bool {
    text.lines()
        .skip_while(|l| l.starts_with("#!") && !l.starts_with("#!["))
        .take(10)
        .any(|l| l.contains("mozilla.org/MPL/2.0"))
}

/// license.template, as a block comment and as `#` comments.
const HEADER_BLOCK: [&str; 3] = [
    "/* This Source Code Form is subject to the terms of the Mozilla Public",
    " * License, v. 2.0. If a copy of the MPL was not distributed with this",
    " * file, You can obtain one at http://mozilla.org/MPL/2.0/. */",
];
const HEADER_HASH: [&str; 3] = [
    "# This Source Code Form is subject to the terms of the Mozilla Public",
    "# License, v. 2.0. If a copy of the MPL was not distributed with this",
    "# file, You can obtain one at http://mozilla.org/MPL/2.0/.",
];

/// Words of a license or copyright notice other than the MPL header.
const OTHER_TERMS: &[&str] = &[
    "copyright",
    "spdx",
    "public domain",
    "licensed under",
    "license, v",
    "apache license",
    "mit license",
    "gnu general public",
];

/// The header inserted at the top of the file, after the lines that must
/// stay first (a shebang, Python's `coding` line, Package.swift's
/// `swift-tools-version`), followed by one blank line. `None` when the file
/// already names another license or a copyright holder: a human decides.
fn header_fix(file: &TextFile) -> Option<Fix> {
    let lines: Vec<&str> = file.text.lines().collect();
    let other_terms = lines.iter().take(10).any(|l| {
        let l = l.to_lowercase();
        OTHER_TERMS.iter().any(|w| l.contains(w))
    });
    if other_terms {
        return None;
    }
    let ext = file.extension();
    let mut at = 0;
    if lines
        .first()
        .is_some_and(|l| l.starts_with("#!") && !l.starts_with("#!["))
    {
        at = 1;
    }
    if ext == "py"
        && lines
            .get(at)
            .is_some_and(|l| l.starts_with('#') && l.contains("coding"))
    {
        at += 1;
    }
    if lines
        .get(at)
        .is_some_and(|l| l.starts_with("// swift-tools-version"))
    {
        at += 1;
    }
    let header = if matches!(ext, "py" | "sh") {
        HEADER_HASH
    } else {
        HEADER_BLOCK
    };
    let mut insert: Vec<String> = header.iter().map(|l| l.to_string()).collect();
    if !lines.get(at).is_some_and(|l| l.trim().is_empty()) {
        insert.push(String::new());
    }
    Some(Fix::insert_before(at + 1, insert))
}

/// Lines of a Rust file whose end is inside a string literal, where trailing
/// whitespace is part of the string.
fn string_lines(file: &RustFile) -> BTreeSet<usize> {
    fn walk(stream: TokenStream, out: &mut BTreeSet<usize>) {
        for tree in stream {
            match tree {
                TokenTree::Group(g) => walk(g.stream(), out),
                TokenTree::Literal(lit) => {
                    let span = lit.span();
                    out.extend(span.start().line..span.end().line);
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(file.ast.to_token_stream(), &mut out);
    out
}

fn whitespace(file: &TextFile, in_strings: &BTreeSet<usize>, out: &mut Findings) {
    let text = &file.text;
    let lines: Vec<&str> = text.split('\n').collect();
    let path = file.path.as_str();
    if let Some(i) = lines.iter().position(|l| l.ends_with('\r')) {
        out.add(
            path,
            i + 1,
            "CRLF line endings: convert the file to LF (`\\n`) endings",
        );
    }
    // Two trailing spaces are a Markdown line break.
    let markdown = file.extension() == "md";
    let trailing: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.trim_end_matches('\r');
            let stripped = l.trim_end_matches([' ', '\t']);
            stripped.len() != l.len()
                && !(markdown && !stripped.is_empty() && l.len() - stripped.len() == 2)
        })
        .map(|(i, _)| i + 1)
        .collect();
    if let Some(first) = trailing.first() {
        let message = format!(
            "trailing whitespace on {} line(s), first here: strip it",
            trailing.len()
        );
        // Not inside a string, not a Markdown line break, not in a CRLF file
        // (the edit would change its line endings).
        let safe = !text.contains('\r')
            && trailing.iter().all(|n| {
                let l = lines[n - 1];
                let stripped = l.trim_end_matches([' ', '\t']);
                !in_strings.contains(n)
                    && !(markdown && !stripped.is_empty() && l.len() - stripped.len() >= 2)
            });
        if safe {
            let edits = trailing
                .iter()
                .map(|n| Edit {
                    line: *n,
                    remove: 1,
                    insert: vec![lines[n - 1].trim_end_matches([' ', '\t']).to_string()],
                })
                .collect();
            out.add_fix(path, *first, message, Fix { edits });
        } else {
            out.add(path, *first, message);
        }
    }
    if !text.ends_with('\n') {
        // Rewriting the last line as a whole line ends it with a newline.
        out.add_fix(
            path,
            lines.len(),
            "no newline at end of file: add a final `\\n`",
            Fix::replace_line(lines.len(), lines[lines.len() - 1]),
        );
    } else {
        // `split` leaves one empty string after the final newline.
        let blank = lines
            .iter()
            .rev()
            .skip(1)
            .take_while(|l| l.trim().is_empty())
            .count();
        if blank > 0 {
            out.add_fix(
                path,
                lines.len() - blank,
                format!(
                    "{blank} blank line(s) at end of file: end the file right after its last line"
                ),
                Fix::delete(lines.len() - blank, blank),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/logins/src/db/row.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/license-header-and-whitespace/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].line, 8);
    }

    /// `(line, remove, insert)` of each edit of a finding's fix.
    fn edits(f: &crate::rule::Finding) -> Vec<(usize, usize, Vec<String>)> {
        let fix = f.fix.as_ref().expect("a fix");
        fix.edits
            .iter()
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn fixes_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/license-header-and-whitespace/dont.rs"),
            )],
        );
        let mut header = lines(
            "/* This Source Code Form is subject to the terms of the Mozilla Public\n * License, v. 2.0. If a copy of the MPL was not distributed with this\n * file, You can obtain one at http://mozilla.org/MPL/2.0/. */\n",
        );
        header.push(String::new());
        assert_eq!(edits(&found[0]), vec![(1, 0, header)]);
        // The two blank lines at the end go.
        assert_eq!(edits(&found[1]), vec![(8, 2, vec![])]);
    }

    #[test]
    fn header_fix_keeps_first_lines_and_other_licenses() {
        let found = check(
            &RULE,
            &[
                ("automation/y.sh", "#!/usr/bin/env bash\n\nset -e\n"),
                (
                    "megazords/ios-rust/Package.swift",
                    "// swift-tools-version:5.9\nimport PackageDescription\n",
                ),
                (
                    "testing/sync-test/src/main.rs",
                    "/* Any copyright is dedicated to the Public Domain\n   http://creativecommons.org/publicdomain/zero/1.0/ */\n",
                ),
            ],
        );
        assert_eq!(found.len(), 3, "{found:#?}");
        // After the shebang, before its blank line.
        assert_eq!(edits(&found[0]), vec![(2, 0, lines(HEADER))]);
        // After `swift-tools-version`, which must stay first.
        let swift = &edits(&found[1])[0];
        assert_eq!((swift.0, swift.1, swift.2.len()), (2, 0, 4));
        assert!(swift.2[0].starts_with("/* This Source Code Form"));
        // Public domain: a human decides.
        assert!(found[2].fix.is_none());
    }

    #[test]
    fn whitespace_fixes() {
        let text = format!("{HEADER}a = 1 \nb = 2\t\nc = 3");
        let found = check(&RULE, &[("automation/x.py", &text)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(
            edits(&found[0]),
            vec![
                (4, 1, vec!["a = 1".to_string()]),
                (5, 1, vec!["b = 2".to_string()])
            ]
        );
        // The last line, rewritten as a whole line, gets its newline.
        assert_eq!(edits(&found[1]), vec![(6, 1, vec!["c = 3".to_string()])]);
        // Trailing spaces inside a string literal are part of the string.
        let rust = "const Q: &str = \"\\\nSELECT \n    a\";\n";
        let found = check(&RULE, &[("tools/x/src/q.rs", rust)]);
        let trailing: Vec<_> = found
            .iter()
            .filter(|f| f.message.starts_with("trailing"))
            .collect();
        assert_eq!(trailing.len(), 1, "{found:#?}");
        assert!(trailing[0].fix.is_none());
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/license-header-and-whitespace/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    const HEADER: &str = "# This Source Code Form is subject to the terms of the Mozilla Public\n# License, v. 2.0. If a copy of the MPL was not distributed with this\n# file, You can obtain one at http://mozilla.org/MPL/2.0/.\n";

    #[test]
    fn header_after_a_shebang() {
        let script = format!("#!/usr/bin/env bash\n{HEADER}set -e\n");
        assert!(check(&RULE, &[("automation/x.sh", &script)]).is_empty());
        let found = check(
            &RULE,
            &[("automation/y.sh", "#!/usr/bin/env bash\nset -e\n")],
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn whitespace_problems() {
        let text = format!("{HEADER}a = 1 \r\nb = 2\n\n\n");
        let found = check(
            &RULE,
            &[("taskcluster/x.yml", &text), ("automation/x.py", &text)],
        );
        // yml: no header needed; CRLF, trailing space, blank lines. py: the same.
        assert_eq!(found.len(), 6, "{found:#?}");
    }

    #[test]
    fn markdown_line_breaks_and_data_files() {
        let files = [
            ("docs/a.md", "first line  \nsecond\n"),
            ("components/rs/dumps/x.json", "{}"),
            ("components/x/fixtures/a.rs", "fn a() {}"),
            ("components/x/src/templates/T.kt", "class T\n"),
        ];
        let found = check(&RULE, &files);
        // The fixture still needs its header.
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, "components/x/fixtures/a.rs");
    }
}
