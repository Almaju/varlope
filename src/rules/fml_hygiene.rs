//! FML: pinned remote imports, backward-compatible manifests, clean templates and fixtures
//!
//! Docs and examples: `rules/fml-hygiene/`.

use regex::Regex;

use crate::rule::{Area, Context, Findings, Kind, Rule};
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
                out.add(
                    &file.path,
                    n,
                    format!("`{key}: {default}` restates the default; delete the line"),
                );
            }
        }
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
            out.add(
                &file.path,
                file.line_of(*start),
                format!("`{{% {kind} {name} %}}` is never used in this template; delete it"),
            );
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
