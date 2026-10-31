//! New behaviour comes with tests in the same PR
//!
//! Docs and examples: `rules/tests-with-behaviour/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::FileChange;
use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "tests-with-behaviour",
    area: Area::Tests,
    title: "New behaviour comes with tests in the same PR",
    kind: Kind::Diff,
    clippy: &[],
    // The rule tells test lines apart itself, on the changed version of each
    // file (the runner only knows the current checkout).
    in_tests: true,
    check,
};

/// From this many new lines of component code, a change needs a test even
/// when it adds no function.
const MIN_LINES: usize = 20;
/// A new function with at least this many lines of body is new behaviour.
const MIN_FN_LINES: usize = 3;

#[derive(Default)]
struct Component {
    /// `(path, line)` of each added line of shipped code.
    code: Vec<(String, usize)>,
    /// `(name, path, line)` of each new function with a real body.
    new_fns: Vec<(String, String, usize)>,
    tested: bool,
}

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    // Lines that moved rather than changed: the same text was removed elsewhere.
    let removed: BTreeSet<&str> = diff
        .files
        .iter()
        .flat_map(|f| f.removed_text())
        .map(str::trim)
        .collect();
    let mut components: BTreeMap<String, Component> = BTreeMap::new();
    let mut integration_tests = false;
    for change in &diff.files {
        let path = change.path.as_str();
        if idioms::is_generated(path) {
            continue;
        }
        if path.starts_with("testing/") && !change.added.is_empty() {
            integration_tests = true;
        }
        let Some(name) = component(path) else {
            continue;
        };
        let entry = components.entry(name.to_string()).or_default();
        if is_test_file(path) {
            entry.tested |= !change.added.is_empty();
            continue;
        }
        if !path.ends_with(".rs") || !path.contains("/src/") || !shipped(path) {
            continue;
        }
        let parsed = parse_new(cx, change);
        let in_test = |line: usize| parsed.as_ref().is_some_and(|f| f.in_test(line));
        let new_code = |text: &str| is_code(text) && !removed.contains(text.trim());
        let mut current: Option<(String, usize, usize)> = None; // name, line, body lines
        let mut previous_line = 0;
        for (line, text) in &change.added {
            if *line != previous_line + 1 {
                finish_fn(&mut current, path, entry);
            }
            previous_line = *line;
            if in_test(*line) {
                entry.tested = true;
                continue;
            }
            if let Some(fn_name) = fn_signature(text) {
                finish_fn(&mut current, path, entry);
                if !fn_name.starts_with("test") && !removed.contains(text.trim()) {
                    current = Some((fn_name, *line, 0));
                }
            } else if let Some((_, _, body)) = current.as_mut() {
                if new_code(text) {
                    *body += 1;
                }
            }
            if new_code(text) {
                entry.code.push((change.path.clone(), *line));
            }
        }
        finish_fn(&mut current, path, entry);
    }
    // Integration tests under `testing/` may cover any component.
    if integration_tests {
        return;
    }
    for (name, c) in components {
        if c.tested {
            continue;
        }
        if let Some((fn_name, path, line)) = c.new_fns.first() {
            let others = match c.new_fns.len() {
                1 => String::new(),
                2 => " (and 1 other new function)".to_string(),
                n => format!(" (and {} other new functions)", n - 1),
            };
            out.add(
                path,
                *line,
                format!("new function `{fn_name}`{others} in components/{name} comes with no test in this PR; add one that covers it"),
            );
        } else if c.code.len() >= MIN_LINES {
            let (path, line) = &c.code[0];
            out.add(
                path,
                *line,
                format!(
                    "{} new lines of code in components/{name} and no test added or changed; add a test for the new behaviour in this PR",
                    c.code.len()
                ),
            );
        }
    }
}

/// Record the function being read if its body is big enough to be behaviour.
fn finish_fn(current: &mut Option<(String, usize, usize)>, path: &str, entry: &mut Component) {
    if let Some((name, line, body)) = current.take() {
        if body >= MIN_FN_LINES {
            entry.new_fns.push((name, path.to_string(), line));
        }
    }
}

/// The name in a line that starts a function: `pub(crate) async fn name(`.
fn fn_signature(text: &str) -> Option<String> {
    let mut rest = text.trim_start();
    for prefix in [
        "pub(crate) ",
        "pub(super) ",
        "pub ",
        "const ",
        "async ",
        "unsafe ",
    ] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `places` for `components/places/src/..`, `support/sql` for support crates.
fn component(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("components/")?;
    let mut parts = rest.splitn(3, '/');
    let first = parts.next()?;
    let len = if first == "support" {
        first.len() + 1 + parts.next()?.len()
    } else {
        first.len()
    };
    Some(&rest[..len])
}

/// Rust tests, Kotlin unit tests and iOS test targets.
fn is_test_file(path: &str) -> bool {
    (path.ends_with(".rs") && is_test_path(path))
        || path.contains("/src/test/")
        || path.contains("/src/androidTest/")
        || (path.ends_with(".swift") && path.contains("Tests"))
}

/// Code that ships: not benchmarks, binaries or build scripts.
fn shipped(path: &str) -> bool {
    !path.contains("/benchmarks/")
        && !path.contains("/bin/")
        && !path.ends_with("/build.rs")
        && !path.ends_with("/main.rs")
}

fn parse_new(cx: &Context, change: &FileChange) -> Option<RustFile> {
    match &change.new_text {
        Some(text) => RustFile::parse(change.path.clone(), text.clone()).ok(),
        None => cx
            .ws
            .rust(&change.path)
            .and_then(|f| RustFile::parse(f.path.clone(), f.text.clone()).ok()),
    }
}

/// A line that does something: not blank, a comment, an attribute or a brace.
fn is_code(text: &str) -> bool {
    let t = text.trim();
    !(t.is_empty()
        || t.starts_with("//")
        || t.starts_with("/*")
        || t.starts_with('*')
        || t.starts_with("#[")
        || t.starts_with("use ")
        || t.starts_with("pub use ")
        || t.starts_with("mod ")
        || t.starts_with("pub mod ")
        || matches!(t, "{" | "}" | "};" | ")" | ");" | "]" | "}," | "})" | "});"))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check_diff, check_unified};

    const SRC: &str = "components/nimbus/src/enrollment.rs";

    fn body(n: usize, prefix: &str) -> String {
        // Statements outside any new function: only the line count matters.
        (0..n)
            .map(|i| format!("    let {prefix}{i} = compute({i});\n"))
            .collect()
    }

    #[test]
    fn flags_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/tests-with-behaviour/dont.diff"),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, SRC);
        assert_eq!(found[0].line, 121);
        assert!(found[0].message.contains("`merge_override`"), "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/tests-with-behaviour/do.diff"),
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn counts_tests_in_cfg_test_modules_of_the_new_file() {
        let old = "fn a() {}\n";
        let new = format!(
            "fn a() {{}}\nfn b() {{\n{}}}\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn t() {{ b(); }}\n}}\n",
            body(25, "x")
        );
        assert!(check_diff(&RULE, &[(SRC, old)], &[(SRC, &new)]).is_empty());
        let untested = format!("fn a() {{}}\nfn b() {{\n{}}}\n", body(25, "x"));
        assert_eq!(
            check_diff(&RULE, &[(SRC, old)], &[(SRC, &untested)]).len(),
            1
        );
    }

    #[test]
    fn ignores_moves_small_changes_and_other_components_tests() {
        let moved = body(30, "y");
        let old = [(SRC, moved.as_str()), ("components/nimbus/src/b.rs", "")];
        let new = [(SRC, ""), ("components/nimbus/src/b.rs", moved.as_str())];
        assert!(check_diff(&RULE, &old, &new).is_empty());
        let small = body(5, "z");
        assert!(check_diff(&RULE, &[(SRC, "")], &[(SRC, &small)]).is_empty());
        let wrapper = "pub fn count(&self) -> usize {\n    self.inner.count()\n}\n";
        assert!(check_diff(&RULE, &[(SRC, "")], &[(SRC, wrapper)]).is_empty());
        let other = [
            (
                SRC,
                "fn merge(a: u8) -> u8 {\n    let b = a + 1;\n    let c = b * 2;\n    c - 1\n}\n",
            ),
            ("components/places/src/tests.rs", "#[test]\nfn t() {}\n"),
        ];
        assert_eq!(check_diff(&RULE, &[], &other).len(), 1);
    }
}
