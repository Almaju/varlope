//! Delete commented-out code
//!
//! Docs and examples: `rules/no-commented-out-code/`.

use crate::comment::Comment;
use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;

pub static RULE: Rule = Rule {
    id: "no-commented-out-code",
    area: Area::DeadCode,
    title: "Delete commented-out code",
    kind: Kind::Code,
    clippy: &[],
    in_tests: true,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        for block in commented_out_blocks(file) {
            let what = if block.code == 1 {
                "a line of commented-out code".to_string()
            } else {
                format!("{} lines of commented-out code", block.code)
            };
            out.add_fix(
                &file.path,
                block.line,
                format!("{what}; delete it (git keeps the history) or say in prose why it matters"),
                deletion(file, block.delete.0, block.delete.1),
            );
        }
    }
}

/// A block of commented-out code.
struct Block {
    /// The first line of code.
    line: usize,
    /// How many of its lines are code.
    code: usize,
    /// The comment lines to delete with it, `(first, count)`: the block and
    /// the empty `//` lines that would be left dangling.
    delete: (usize, usize),
}

/// Delete `count` lines from `first`, and a blank line that would then be
/// doubled or sit at the start or end of a block (rustfmt removes those).
fn deletion(file: &RustFile, first: usize, count: usize) -> Fix {
    let lines: Vec<&str> = file.text.lines().collect();
    let blank = |n: usize| n >= 1 && lines.get(n - 1).is_some_and(|l: &&str| l.trim().is_empty());
    let before = first
        .checked_sub(1)
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| lines.get(i));
    let after = lines.get(first + count - 1);
    let opens = before.is_none_or(|l| l.trim_end().ends_with(['{', '(', '[']));
    let closes = after.is_none_or(|l| l.trim_start().starts_with(['}', ')', ']']));
    if blank(first + count) && (blank(first - 1) || opens) {
        Fix::delete(first, count + 1)
    } else if blank(first - 1) && closes {
        Fix::delete(first - 1, count + 1)
    } else {
        Fix::delete(first, count)
    }
}

/// Each block of commented-out code.
fn commented_out_blocks(file: &RustFile) -> Vec<Block> {
    let mut out = Vec::new();
    for run in comment_runs(file) {
        let texts: Vec<&str> = run
            .iter()
            .map(|c| strip_trailing_comment(&c.text))
            .collect();
        let code = code_lines(&texts);
        let mut i = 0;
        while i < code.len() {
            if !code[i] {
                i += 1;
                continue;
            }
            let start = i;
            while i < code.len() && (code[i] || texts[i].is_empty()) {
                i += 1;
            }
            let count = (start..i).filter(|&k| code[k]).count();
            if !explained(file, &run, &texts, start) {
                // At the end of the run, empty `//` lines above would dangle.
                let mut first = start;
                if i == run.len() {
                    while first > 0 && texts[first - 1].is_empty() {
                        first -= 1;
                    }
                }
                out.push(Block {
                    line: run[start].line,
                    code: count,
                    delete: (run[first].line, i - first),
                });
            }
        }
    }
    out
}

/// Whether prose right above a block of code says why it is commented out
/// (`// TODO: re-enable once ..` followed by the code).
fn explained(file: &RustFile, run: &[&Comment], texts: &[&str], start: usize) -> bool {
    if start > 0 {
        return !texts[start - 1].is_empty();
    }
    let mut line = run[start].line;
    for _ in 0..2 {
        line -= 1;
        if line == 0 {
            return false;
        }
        let above = file.line(line).trim();
        if above.is_empty() {
            continue;
        }
        return above.starts_with("//") && !above.starts_with("///");
    }
    false
}

/// The text of a comment line without a trailing `// note` of its own.
fn strip_trailing_comment(text: &str) -> &str {
    let text = text.trim();
    match text.find(" //") {
        Some(at) if !text[..at].contains('"') => text[..at].trim_end(),
        _ => text,
    }
}

/// Runs of `//` comments on consecutive lines, each alone on its line.
fn comment_runs(file: &RustFile) -> Vec<Vec<&Comment>> {
    let mut runs: Vec<Vec<&Comment>> = Vec::new();
    let mut previous: Option<&Comment> = None;
    for comment in file.comments.iter() {
        let own_line = file.line(comment.line).trim_start().starts_with("//");
        if !comment.is_line() || !own_line {
            previous = None;
            continue;
        }
        match runs.last_mut() {
            Some(run) if comment.continues(previous) => run.push(comment),
            _ => runs.push(vec![comment]),
        }
        previous = Some(comment);
    }
    runs
}

/// Which lines of a comment run are code. A line counts when it, or the
/// smallest window of lines starting at it with balanced brackets, parses as
/// Rust statements or items.
fn code_lines(texts: &[&str]) -> Vec<bool> {
    let mut code = vec![false; texts.len()];
    if prose_run(texts) {
        return code;
    }
    let mut i = 0;
    while i < texts.len() {
        if !looks_like_code(texts[i]) {
            i += 1;
            continue;
        }
        let mut matched = None;
        let mut depth = 0i32;
        for (j, text) in texts.iter().enumerate().skip(i) {
            if prose(text) {
                break;
            }
            depth += bracket_depth(text);
            if depth < 0 {
                break;
            }
            if depth == 0 && parses(&texts[i..=j].join("\n")) {
                matched = Some(j);
                break;
            }
            if j - i > 40 {
                break;
            }
        }
        match matched {
            Some(j) => {
                code[i..=j].iter_mut().for_each(|c| *c = true);
                i = j + 1;
            }
            None => i += 1,
        }
    }
    code
}

/// Comment runs that are documentation even when they contain code: examples
/// introduced by prose ("for example:", "we used to do:").
fn prose_run(texts: &[&str]) -> bool {
    texts.iter().any(|t| {
        let lower = t.to_lowercase();
        t.ends_with(':')
            || lower.contains("e.g.")
            || lower.contains("example")
            || lower.contains("```")
            || lower.starts_with("note")
    })
}

/// Lines that are never code: notes, links and markdown.
fn prose(text: &str) -> bool {
    let upper = text.to_uppercase();
    ["TODO", "FIXME", "XXX", "NOTE", "SAFETY", "HACK"]
        .iter()
        .any(|w| upper.starts_with(w))
        || text.contains("://")
        || text.contains('`')
        || text.starts_with("- ")
        || text.starts_with("* ")
}

/// A quick filter before trying to parse: the shapes commented-out code takes.
fn looks_like_code(text: &str) -> bool {
    if prose(text) {
        return false;
    }
    const STARTS: &[&str] = &[
        "let ", "fn ", "use ", "pub ", "if ", "for ", "while ", "match ", "return ", "impl ",
        "struct ", "enum ", "mod ", "const ", "static ", "#[", "assert", "self.", "Ok(", "Err(",
    ];
    STARTS.iter().any(|s| text.starts_with(s))
        || [";", "{", ".unwrap()", "?;", ");"]
            .iter()
            .any(|e| text.ends_with(e))
}

fn bracket_depth(text: &str) -> i32 {
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Whether `text` parses as items or statements with some code punctuation
/// (so that `// return early;` stays prose).
fn parses(text: &str) -> bool {
    let has_syntax = ["(", "=", "::", "!", "[", "{", "&", "."]
        .iter()
        .any(|p| text.contains(p));
    if !has_syntax {
        return false;
    }
    if syn::parse_str::<syn::File>(text).is_ok_and(|f| !f.items.is_empty()) {
        return true;
    }
    let block = format!("{{\n{text}\n}}");
    match syn::parse_str::<syn::Block>(&block) {
        Ok(b) => {
            // A lone path or literal (`// foo`) is a word, not code.
            !b.stmts.is_empty()
                && b.stmts.iter().all(|s| match s {
                    syn::Stmt::Expr(e, semi) => {
                        semi.is_some()
                            || matches!(
                                e,
                                syn::Expr::If(_)
                                    | syn::Expr::ForLoop(_)
                                    | syn::Expr::Match(_)
                                    | syn::Expr::While(_)
                                    | syn::Expr::Block(_)
                            )
                    }
                    _ => true,
                })
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/no-commented-out-code/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
        assert!(found[0].message.starts_with("2 lines"));
    }

    /// `(line, remove)` of a finding's fix, which only deletes.
    fn deleted(f: &crate::rule::Finding) -> (usize, usize) {
        let fix = f.fix.as_ref().expect("a fix");
        assert_eq!(fix.edits.len(), 1, "{fix:#?}");
        assert!(fix.edits[0].insert.is_empty());
        (fix.edits[0].line, fix.edits[0].remove)
    }

    #[test]
    fn fix_deletes_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/no-commented-out-code/dont.rs"),
            )],
        );
        assert_eq!(deleted(&found[0]), (4, 2));
    }

    #[test]
    fn fix_leaves_no_doubled_or_dangling_blank_lines() {
        // Between blank lines: one of them goes too.
        let src = "fn f() {\n    a();\n\n    // b();\n\n    c();\n}\n";
        assert_eq!(deleted(&check(&RULE, &[(PATH, src)])[0]), (4, 2));
        // Right before the closing brace: the blank line above goes.
        let src = "fn f() {\n    a();\n\n    // b();\n}\n";
        assert_eq!(deleted(&check(&RULE, &[(PATH, src)])[0]), (3, 2));
        // At the end of a comment, with the empty `//` line above it.
        let src = "fn f() {\n    // Why this is slow.\n    //\n    // b();\n    a();\n}\n";
        assert_eq!(deleted(&check(&RULE, &[(PATH, src)])[0]), (3, 2));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/no-commented-out-code/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_multi_line_blocks() {
        let src = "fn f() {\n    // if store.is_empty() {\n    //     return Ok(());\n    // }\n    g();\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 2);
        assert!(found[0].message.starts_with("3 lines"));
    }

    #[test]
    fn ignores_prose_and_examples() {
        let src = "fn f() {\n\
            // if the user is signed out, return early;\n\
            // return early;\n\
            // TODO: let x = 1;\n\
            // For example:\n\
            //   let store = Store::new(path)?;\n\
            // See https://example.com/a;b\n\
            /// let doc = test();\n\
            // Call `store.flush()` first;\n\
            g(); // h();\n\
            // TODO: re-enable once the server supports it.\n\
            // store.prune(&conn)?;\n\
        }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
