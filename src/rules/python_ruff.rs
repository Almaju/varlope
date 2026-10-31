//! Python automation is formatted and linted (ruff) and exits non-zero on failure
//!
//! Docs and examples: `rules/python-ruff/`.

use std::sync::LazyLock;

use regex::Regex;

use crate::helpers::docs_deps::{hash_comment_start, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::TextFile;
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "python-ruff",
    area: Area::Scripts,
    title: "Python automation is formatted and linted (ruff) and exits non-zero on failure",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// `x in d.keys()` (ruff SIM118).
static KEYS_MEMBERSHIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bin\s+[\w.\[\]'\x22]+\.keys\(\)").expect("valid regex"));

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files_with_ext(&["py"]) {
        if is_out_of_scope(&file.path) || is_test_path(&file.path) {
            continue;
        }
        let code = code_lines(file);
        main_exit_status(file, &code, out);
        unchecked_subprocess(file, &code, out);
        for (line, text) in &code {
            // The comment after the code, kept as it is.
            let comment = &file.line(*line)[text.len()..];
            if KEYS_MEMBERSHIP.is_match(text) {
                let fixed = KEYS_MEMBERSHIP.replace_all(text, |c: &regex::Captures| {
                    c[0].trim_end_matches(".keys()").to_string()
                });
                out.add_fix(
                    &file.path,
                    *line,
                    "`in d.keys()` builds a view for nothing: write `in d` (ruff SIM118)",
                    Fix::replace_line(*line, format!("{fixed}{comment}")),
                );
            }
            if text.contains("lru_cache(maxsize=None)") {
                let message = "`lru_cache(maxsize=None)` is `functools.cache` (ruff UP033)";
                // A bare `lru_cache` would need its import changed too.
                if text.matches("lru_cache(maxsize=None)").count()
                    == text.matches("functools.lru_cache(maxsize=None)").count()
                {
                    let fixed =
                        text.replace("functools.lru_cache(maxsize=None)", "functools.cache");
                    out.add_fix(
                        &file.path,
                        *line,
                        message,
                        Fix::replace_line(*line, format!("{fixed}{comment}")),
                    );
                } else {
                    out.add(&file.path, *line, message);
                }
            }
        }
    }
}

/// `(line, code without its comment)` for each line.
fn code_lines(file: &TextFile) -> Vec<(usize, String)> {
    file.lines()
        .map(|(n, l)| {
            let code = match hash_comment_start(l) {
                Some(at) => &l[..at],
                None => l,
            };
            (n, code.trim_end().to_string())
        })
        .collect()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `if __name__ == "__main__": main()` where `main` returns a status: the
/// status is dropped and the process exits 0 whatever happened.
fn main_exit_status(file: &TextFile, code: &[(usize, String)], out: &mut Findings) {
    let Some(def) = code.iter().position(|(_, l)| l.starts_with("def main(")) else {
        return;
    };
    let mut returns_value = false;
    let mut nested: Option<usize> = None;
    for (_, line) in &code[def + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        let depth = indent(line);
        if depth == 0 {
            break;
        }
        if nested.is_some_and(|d| depth > d) {
            continue;
        }
        nested = None;
        let t = line.trim();
        if t.starts_with("def ") || t.starts_with("async def ") || t.starts_with("lambda") {
            nested = Some(depth);
        } else if let Some(value) = t.strip_prefix("return ") {
            returns_value |= value.trim() != "None";
        }
    }
    if !returns_value {
        return;
    }
    let Some(guard) = code
        .iter()
        .position(|(_, l)| l.starts_with("if __name__ ==") || l.starts_with("if \"__main__\" =="))
    else {
        return;
    };
    for (line, text) in &code[guard + 1..] {
        if !text.trim().is_empty() && indent(text) == 0 {
            break;
        }
        let t = text.trim();
        if t.starts_with("main(") && t.ends_with(')') {
            let message = "`main()` returns an exit status that is dropped, so the script exits 0 on failure: call `sys.exit(main())`";
            // Only when `sys` is imported: adding an import is not this line's job.
            if imports_sys(code) {
                let indent = &text[..indent(text)];
                let comment = &file.line(*line)[text.len()..];
                out.add_fix(
                    &file.path,
                    *line,
                    message,
                    Fix::replace_line(*line, format!("{indent}sys.exit({t}){comment}")),
                );
            } else {
                out.add(&file.path, *line, message);
            }
        }
    }
}

/// A top-level `import sys` (or `import os, sys`).
fn imports_sys(code: &[(usize, String)]) -> bool {
    code.iter().any(|(_, l)| {
        l.strip_prefix("import ")
            .is_some_and(|m| m.split(',').any(|m| m.trim() == "sys"))
    })
}

/// `subprocess.run(...)` with no `check=` whose result is thrown away, so a
/// failing command goes unnoticed (ruff PLW1510).
fn unchecked_subprocess(file: &TextFile, code: &[(usize, String)], out: &mut Findings) {
    let joined: String = code
        .iter()
        .map(|(_, l)| l.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut search = 0;
    while let Some(found) = joined[search..].find("subprocess.run(") {
        let start = search + found;
        let open = start + "subprocess.run".len();
        search = open;
        let Some(close) = matching_paren(&joined, open) else {
            break;
        };
        let args = &joined[open..close];
        if args.contains("check=") || args.contains("**") {
            continue;
        }
        let line_start = joined[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let before = joined[line_start..start].trim();
        if joined[close + 1..].trim_start().starts_with('.') {
            continue; // `.returncode`, `.stdout`: the result is read
        }
        let discarded = before.is_empty();
        let assigned_unchecked = before
            .strip_suffix('=')
            .map(|name| name.trim())
            .filter(|name| name.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .is_some_and(|name| !joined[close..].contains(&format!("{name}.returncode")));
        if discarded || assigned_unchecked {
            let line = 1 + joined[..start].matches('\n').count();
            out.add(
                &file.path,
                code.get(line - 1).map(|(n, _)| *n).unwrap_or(line),
                "`subprocess.run` without `check=` ignores a failing command: pass `check=True`, or `check=False` and read `returncode` (ruff PLW1510)",
            );
        }
    }
}

/// The index of the `)` closing the `(` at `open`, skipping strings.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in text[open..].char_indices() {
        match quote {
            Some(q) => {
                if c == q && prev != '\\' {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + i);
                    }
                }
                _ => {}
            },
        }
        prev = c;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "automation/symbols-generation/upload_symbols.py";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/python-ruff/dont.py"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 13);
    }

    /// `(line, remove, insert)` of each edit of a finding's fix.
    fn edits(f: &crate::rule::Finding) -> Vec<(usize, usize, Vec<String>)> {
        let fix = f.fix.as_ref().expect("a fix");
        fix.edits
            .iter()
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    #[test]
    fn fixes_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/python-ruff/dont.py"))],
        );
        assert_eq!(
            edits(&found[0]),
            vec![(
                13,
                1,
                vec![
                    "    sys.exit(main())  # flagged: the status is dropped, so the task passes anyway"
                        .to_string()
                ]
            )]
        );
        // Without `import sys`, the call alone would not run.
        let text = include_str!("../../rules/python-ruff/dont.py").replace("import sys\n", "");
        assert!(check(&RULE, &[(PATH, &text)])[0].fix.is_none());
    }

    #[test]
    fn fix_keys_and_cache() {
        let text = "import functools\n\n@functools.lru_cache(maxsize=None)\ndef f(d, k):\n    return k in d.keys() or k in e[\"x\"].keys()  # in d.keys()\n\n@lru_cache(maxsize=None)\ndef g():\n    pass\n";
        let found = check(&RULE, &[("taskcluster/a.py", text)]);
        assert_eq!(found.len(), 3, "{found:#?}");
        assert_eq!(
            edits(&found[0]),
            vec![(3, 1, vec!["@functools.cache".to_string()])]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(
                5,
                1,
                vec!["    return k in d or k in e[\"x\"]  # in d.keys()".to_string()]
            )]
        );
        // A bare `lru_cache` also needs its import changed.
        assert!(found[2].fix.is_none());
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/python-ruff/do.py"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn main_without_a_status_is_fine() {
        let text = "def main():\n    def helper():\n        return 1\n    helper()\n    return None\n\n\nif __name__ == \"__main__\":\n    main()\n";
        assert!(check(&RULE, &[("tools/a.py", text)]).is_empty());
    }

    #[test]
    fn subprocess_results() {
        let text = "import subprocess\n\nsubprocess.run([\"cargo\", \"build\"])\nok = subprocess.run([\"x\"]).returncode == 0\nr = subprocess.run(\n    [\"y\"],\n    cwd=d,\n)\nif r.returncode:\n    pass\nsubprocess.run([\"z\"], check=True)\nreturn subprocess.run(*args, **kwargs)\n";
        let found = check(&RULE, &[("tools/a.py", text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn keys_membership_and_cache() {
        let text = "import functools\n\n@functools.lru_cache(maxsize=None)\ndef f(d, k):\n    return k in d.keys()  # in d.keys() in a comment\n";
        let found = check(&RULE, &[("taskcluster/a.py", text)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(check(&RULE, &[("megazords/x/tests/t.py", text)]).is_empty());
    }
}
