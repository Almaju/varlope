//! Documented exceptions: `varlope: allow(rule-id, ...) reason`.
//!
//! The directive can sit in any comment style (`//`, `#`, `--`, `<!--`). On a
//! line of its own it covers the next item (in Rust: the whole item, field,
//! statement or method that starts on the next code line; elsewhere: the next
//! non-comment line). At the end of a code line it covers that line.
//! `varlope: allow-file(rule-id) reason` covers the whole file. The reason is
//! required: an allow comment without one silences nothing and is reported.

use std::ops::RangeInclusive;

const PREFIX: &str = "varlope:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowance {
    pub line: usize,
    pub rules: Vec<String>,
    pub reason: String,
    pub scope: RangeInclusive<usize>,
}

/// An allow comment that does not work: no reason, or no closing `)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub message: String,
}

/// A region an allowance can attach to (Rust only): an item, field, method...
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Allowances {
    items: Vec<Allowance>,
    problems: Vec<Problem>,
}

impl Allowances {
    /// Find every directive in `text`. `scopes` (Rust only) lets an allowance
    /// above an item cover all of it.
    pub fn parse(text: &str, scopes: &[Scope]) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Allowances::default();
        for (index, line_text) in lines.iter().enumerate() {
            let line = index + 1;
            let Some(at) = line_text.find(PREFIX) else {
                continue;
            };
            let before = line_text[..at].trim();
            if !is_comment_opener(before) {
                continue;
            }
            let directive = line_text[at + PREFIX.len()..].trim();
            let directive = directive
                .trim_end_matches("-->")
                .trim_end_matches("*/")
                .trim();
            let (file_wide, rest) = if let Some(rest) = directive.strip_prefix("allow-file") {
                (true, rest)
            } else if let Some(rest) = directive.strip_prefix("allow") {
                (false, rest)
            } else {
                continue;
            };
            let rest = rest.trim_start();
            let (Some(rest), Some(close)) = (rest.strip_prefix('('), rest.find(')')) else {
                out.problems.push(Problem {
                    line,
                    message: "allow comment is missing `(rule-id)`".into(),
                });
                continue;
            };
            let names = &rest[..close - 1];
            let rules: Vec<String> = names
                .split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect();
            let reason = rest[close..].trim().trim_start_matches([':', '-']).trim();
            if reason.split_whitespace().count() < 2 {
                out.problems.push(Problem {
                    line,
                    message: "allow comment needs a reason: say why the rule does not apply here"
                        .into(),
                });
                continue;
            }
            let own_line = before_comment(line_text).trim().is_empty();
            let scope = if file_wide {
                1..=usize::MAX
            } else if !own_line {
                line..=line
            } else {
                match next_code_line(line, &lines) {
                    Some(target) => {
                        let end = scopes
                            .iter()
                            .filter(|s| s.start_line == target)
                            .map(|s| s.end_line)
                            .max()
                            .unwrap_or(target);
                        target..=end
                    }
                    None => line..=line,
                }
            };
            out.items.push(Allowance {
                line,
                rules,
                reason: reason.to_string(),
                scope,
            });
        }
        out
    }

    pub fn covers(&self, rule: &str, line: usize) -> bool {
        self.items
            .iter()
            .any(|a| a.scope.contains(&line) && a.rules.iter().any(|r| r == rule))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Allowance> {
        self.items.iter()
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }
}

/// The text before the comment that holds the directive ends with a comment
/// opener: `//`, `#`, `--`, `<!--`, `/*`.
fn is_comment_opener(before: &str) -> bool {
    ["//", "#", "--", "<!--", "/*", "*"]
        .iter()
        .any(|opener| before.ends_with(opener))
}

/// The code on a line before its comment, roughly.
fn before_comment(line: &str) -> &str {
    let cut = ["//", "#", "--", "<!--", "/*"]
        .iter()
        .filter_map(|opener| line.find(opener))
        .min()
        .unwrap_or(line.len());
    &line[..cut]
}

/// The first line after `line` that holds code: not blank, not a comment, not
/// an attribute.
fn next_code_line(line: usize, lines: &[&str]) -> Option<usize> {
    (line + 1..=lines.len()).find(|&candidate| {
        let text = lines[candidate - 1].trim();
        !(text.is_empty()
            || text.starts_with("//")
            || text.starts_with('#') && !text.starts_with("#!") && !text.starts_with("#[")
            || text.starts_with("--")
            || text.starts_with("<!--")
            || text.starts_with("#[")
            || text.starts_with("#!["))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_the_next_item() {
        let text = "// varlope: allow(no-unwrap) startup only, can't fail\nfn a() {\n  x.unwrap();\n}\nfn b() {}\n";
        let a = Allowances::parse(
            text,
            &[Scope {
                start_line: 2,
                end_line: 4,
            }],
        );
        assert!(a.covers("no-unwrap", 3));
        assert!(!a.covers("no-unwrap", 5));
        assert!(!a.covers("other", 3));
    }

    #[test]
    fn trailing_comment_covers_its_line() {
        let text = "let a = b.unwrap(); // varlope: allow(no-unwrap) checked just above\nlet c = d.unwrap();\n";
        let a = Allowances::parse(text, &[]);
        assert!(a.covers("no-unwrap", 1));
        assert!(!a.covers("no-unwrap", 2));
    }

    #[test]
    fn hash_comments_and_file_wide() {
        let text =
            "# varlope: allow-file(shell-strict-shellcheck) sourced by other scripts\necho hi\n";
        let a = Allowances::parse(text, &[]);
        assert!(a.covers("shell-strict-shellcheck", 2));
    }

    #[test]
    fn reason_is_required() {
        let a = Allowances::parse("// varlope: allow(no-unwrap)\nfn a() {}\n", &[]);
        assert!(!a.covers("no-unwrap", 2));
        assert_eq!(a.problems().len(), 1);
    }

    #[test]
    fn ignores_the_word_outside_comments() {
        let a = Allowances::parse("let s = \"varlope: allow(x) because reasons\";\n", &[]);
        assert_eq!(a.iter().count(), 0);
    }
}
