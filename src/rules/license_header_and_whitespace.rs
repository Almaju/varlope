//! Every source file starts with the MPL-2.0 header and ends with a newline
//!
//! Docs and examples: `rules/license-header-and-whitespace/`.

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;

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
            out.add(
                path,
                1,
                "no MPL-2.0 header: start the file with the license.template text, in this file's comment syntax",
            );
        }
        if WHITESPACE_EXTS.contains(&ext) && !path.split('/').any(|p| p == "fixtures") {
            whitespace(file, out);
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

fn whitespace(file: &TextFile, out: &mut Findings) {
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
        out.add(
            path,
            *first,
            format!(
                "trailing whitespace on {} line(s), first here: strip it",
                trailing.len()
            ),
        );
    }
    if !text.ends_with('\n') {
        out.add(
            path,
            lines.len(),
            "no newline at end of file: add a final `\\n`",
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
            out.add(
                path,
                lines.len() - blank,
                format!(
                    "{blank} blank line(s) at end of file: end the file right after its last line"
                ),
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
