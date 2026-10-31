//! FML: pinned remote imports, backward-compatible manifests, clean templates and fixtures
//!
//! Docs and examples: `rules/fml-hygiene/`.

use regex::Regex;

use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "fml-hygiene",
    area: Area::NimbusGlean,
    title:
        "FML: pinned remote imports, backward-compatible manifests, clean templates and fixtures",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Feature keys and the default value that restating adds nothing.
const DEFAULTS: &[(&str, &str)] = &[("allow-coenrollment", "false")];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files_with_ext(&["yaml", "yml"]) {
        if is_fml(file) && !deliberately_odd(&file.path) {
            restated_defaults(file, out);
        }
    }
    for file in cx.ws.files() {
        if file.path.contains("nimbus-fml/src/backends/") && file.path.contains("/templates/") {
            unused_template_bindings(file, out);
        }
    }
}

/// A Feature Manifest: has a top-level `features:` and FML's other keys.
fn is_fml(file: &TextFile) -> bool {
    file.path.ends_with(".fml.yaml")
        || (file.text.lines().any(|l| l.starts_with("features:"))
            && file.text.lines().any(|l| {
                l.starts_with("about:")
                    || l.starts_with("channels:")
                    || l.trim_start().starts_with("variables:")
            }))
}

/// Fixtures that test invalid or legacy syntax on purpose.
fn deliberately_odd(path: &str) -> bool {
    path.contains("/lints/") || path.contains("/invalid/") || path.contains("broken")
}

/// `allow-coenrollment: false` and other keys set to what they default to.
fn restated_defaults(file: &TextFile, out: &mut Findings) {
    for (n, line) in file.lines() {
        let code = line.split(" #").next().unwrap_or(line).trim();
        for (key, default) in DEFAULTS {
            let Some(value) = code
                .strip_prefix(key)
                .and_then(|r| r.trim_start().strip_prefix(':'))
            else {
                continue;
            };
            if value.trim().trim_matches('"') == *default {
                out.add_fix(
                    &file.path,
                    n,
                    format!("`{key}: {default}` restates the default; delete the line"),
                    delete_line(file, n),
                );
            }
        }
    }
}

/// Delete line `n`, with a blank line next to it that would otherwise be
/// doubled or end the file.
fn delete_line(file: &TextFile, n: usize) -> Fix {
    let lines: Vec<&str> = file.text.lines().collect();
    let blank = |i: usize| i >= 1 && lines.get(i - 1).is_some_and(|l| l.trim().is_empty());
    if blank(n - 1) && blank(n + 1) {
        Fix::delete(n, 2)
    } else if blank(n - 1) && n == lines.len() {
        Fix::delete(n - 1, 2)
    } else {
        Fix::delete(n, 1)
    }
}

/// `{% let x = … %}` and `{% import "…" as m %}` that the template never uses.
fn unused_template_bindings(file: &TextFile, out: &mut Findings) {
    let tag = Regex::new(r"(?s)\{\{.*?\}\}|\{%.*?%\}").expect("valid regex");
    let binding = Regex::new(r#"^\{%-?\s*(?:let\s+(\w+)\s*=|import\s+"[^"]*"\s+as\s+(\w+))"#)
        .expect("valid regex");
    let tags: Vec<(usize, &str)> = tag
        .find_iter(&file.text)
        .map(|m| (m.start(), m.as_str()))
        .collect();
    for (i, (start, text)) in tags.iter().enumerate() {
        let Some(c) = binding.captures(text) else {
            continue;
        };
        let (name, is_import) = match (c.get(1), c.get(2)) {
            (Some(n), _) => (n.as_str(), false),
            (_, Some(n)) => (n.as_str(), true),
            _ => continue,
        };
        let usage = if is_import {
            Regex::new(&format!(r"\b{name}::"))
        } else {
            Regex::new(&format!(r"\b{name}\b"))
        }
        .expect("valid regex");
        // A `let` is used in later tags (or in its own value, `let x = x|f`
        // excluded); an import anywhere.
        let used = tags
            .iter()
            .enumerate()
            .any(|(j, (_, other))| j != i && (is_import || j > i) && usage.is_match(other));
        if !used {
            let kind = if is_import { "import" } else { "let" };
            let line = file.line_of(*start);
            let message =
                format!("`{{% {kind} {name} %}}` is never used in this template; delete it");
            // Delete the tag's line when it holds nothing else and does not
            // trim the whitespace after it (`-%}`), so the generated code only
            // loses an empty line.
            if file.line(line).trim() == *text && !text.ends_with("-%}") {
                out.add_fix(&file.path, line, message, delete_line(file, line));
            } else {
                out.add(&file.path, line, message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str =
        "components/support/nimbus-fml/fixtures/fe/importing/coenrolling/ui.fml.yaml";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/fml-hygiene/dont.yaml"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
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
            &[(PATH, include_str!("../../rules/fml-hygiene/dont.yaml"))],
        );
        assert_eq!(edits(&found[0]), vec![(4, 1, vec![])]);
    }

    #[test]
    fn fix_deletes_unused_bindings_alone_on_their_line() {
        let template = "{%- let unused = 1 %}\n{%- let trims = 2 -%}\nclass A {}\n\n{% import \"macros.kt\" as kt %}\n";
        let path = "components/support/nimbus-fml/src/backends/kotlin/templates/A.kt";
        let found = check(&RULE, &[(path, template)]);
        assert_eq!(found.len(), 3, "{found:#?}");
        assert_eq!(edits(&found[0]), vec![(1, 1, vec![])]);
        // `-%}` trims what follows: deleting it would change the output.
        assert!(found[1].fix.is_none());
        // The last line, with the blank line that would end the file.
        assert_eq!(edits(&found[2]), vec![(4, 2, vec![])]);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/fml-hygiene/do.yaml"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_lint_fixtures_and_other_yaml() {
        let yaml = "features:\n  a:\n    allow-coenrollment: false\n    variables: {}\n";
        assert!(check(
            &RULE,
            &[(
                "components/support/nimbus-fml/fixtures/fe/lints/x.yaml",
                yaml
            )]
        )
        .is_empty());
        assert!(check(
            &RULE,
            &[("components/x/metrics.yaml", "allow-coenrollment: false\n")]
        )
        .is_empty());
    }

    #[test]
    fn flags_unused_template_bindings() {
        let template = "{%- import \"macros.kt\" as kt %}\n{%- let inner = self.inner() %}\n{%- let unused = inner.name() %}\nclass {{ inner.name() }} {\n    {% call kt::render(inner) %}\n}\n";
        let path = "components/support/nimbus-fml/src/backends/kotlin/templates/ObjectTemplate.kt";
        let found = check(&RULE, &[(path, template)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
        let no_macros = template.replace("{% call kt::render(inner) %}", "");
        assert_eq!(check(&RULE, &[(path, &no_macros)]).len(), 2);
    }
}
