//! `report_error!` labels are component-prefixed, unique, and not on hot paths
//!
//! Docs and examples: `rules/error-report-labels/`.
//!
//! A label is the first argument of `report_error!` or of
//! `ErrorHandling::report_error(..)`; Sentry groups reports by it. On the
//! current code the rule flags labels that are not kebab-case or that start
//! with another component's name (copy-paste from another crate), and
//! reports or breadcrumbs inside loops over incoming records or rows.
//! Renaming an existing label splits its Sentry history, so a label without
//! any component prefix is only flagged on lines a PR adds (with `--base`).
//! The same label in several functions of one crate is left alone: it is
//! usually one bucket on purpose (`tabs-read-remote`).

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "error-report-labels",
    area: Area::Errors,
    title: "`report_error!` labels are component-prefixed, unique, and not on hot paths",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Prefixes in use before this rule, accepted for their Sentry history.
const PREFIX_ALIASES: &[(&str, &str)] = &[
    ("fxa-client", "fxaclient"),
    ("fxa-client", "fxa"),
    ("fxcontainers", "containers"),
];

/// Words in a loop's iterable that mean "once per record".
const PER_RECORD: &[&str] = &[
    "record",
    "row",
    "incoming",
    "inbound",
    "bso",
    "item",
    "entr",
    "visit",
    "change",
    "collision",
    "experiment",
    "enrollment",
    "login",
    "message",
    "payload",
    "guid",
];

/// Iterator adapters whose closure runs once per item.
const PER_ITEM: &[&str] = &[
    "map",
    "for_each",
    "filter_map",
    "flat_map",
    "filter",
    "try_for_each",
    "and_then",
];

struct Label {
    path: String,
    line: usize,
    column: usize,
    text: String,
}

fn check(cx: &Context, out: &mut Findings) {
    // Component names that labels may start with.
    let components: BTreeSet<String> = cx
        .ws
        .rust_files()
        .filter_map(|f| errors::component_of(&f.path))
        .map(|c| c.replace('_', "-"))
        .collect();
    let mut labels = Vec::new();
    for file in errors::shipped_files(cx.ws) {
        if file.path.starts_with("components/support/error/") {
            continue; // the macros and their forwarding code
        }
        let support = TestSupport::of(file);
        let mut v = Reports {
            path: &file.path,
            loops: 0,
            labels: Vec::new(),
            in_loop: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, column, what) in v.in_loop {
            if !support.contains(line) {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("`{what}` inside a loop over records sends one per record; count the failures and report once after the loop"),
                );
            }
        }
        labels.extend(v.labels.into_iter().filter(|l| !support.contains(l.line)));
    }

    let added: BTreeSet<(&str, usize)> = cx
        .diff
        .map(|d| {
            d.files
                .iter()
                .flat_map(|f| f.added.iter().map(move |(n, _)| (f.path.as_str(), *n)))
                .collect()
        })
        .unwrap_or_default();

    for l in &labels {
        let component = errors::component_of(&l.path)
            .unwrap_or_default()
            .replace('_', "-");
        let own_prefixes: Vec<String> = std::iter::once(component.clone())
            .chain(
                PREFIX_ALIASES
                    .iter()
                    .filter(|(c, _)| *c == component)
                    .map(|(_, alias)| alias.to_string()),
            )
            .collect();
        let has_own_prefix = own_prefixes
            .iter()
            .any(|p| l.text.starts_with(&format!("{p}-")));
        let own = |label: &str| {
            own_prefixes
                .iter()
                .any(|p| label.starts_with(&format!("{p}-")))
        };
        // The message, and the label to suggest when it is mechanical.
        let (message, renamed) = if !is_kebab(&l.text) {
            // `tabs-get_unsent` -> `tabs-get-unsent`; a label without the
            // component's prefix needs a name chosen.
            let kebab = kebab(&l.text);
            (
                Some(format!(
                    "report label `{}` is not kebab-case; use a short `{component}-…` label and put details in the message",
                    l.text
                )),
                Some(kebab).filter(|k| is_kebab(k) && own(k)),
            )
        } else if let Some(other) = components
            .iter()
            .filter(|c| **c != component && l.text.starts_with(&format!("{c}-")))
            .find(|_| !has_own_prefix)
        {
            (
                Some(format!(
                    "report label `{}` names the `{other}` component but is reported from `{component}`; use a `{component}-…` label",
                    l.text
                )),
                Some(format!("{component}-{}", &l.text[other.len() + 1..])),
            )
        } else if !has_own_prefix && added.contains(&(l.path.as_str(), l.line)) {
            (
                Some(format!(
                    "start report label `{}` with `{component}-` so Sentry shows where it comes from",
                    l.text
                )),
                Some(format!("{component}-{}", l.text)),
            )
        } else {
            (None, None)
        };
        if let Some(message) = message {
            out.add_at(&l.path, l.line, l.column, message);
            let fix = renamed
                .filter(|_| !component.is_empty())
                .and_then(|r| cx.ws.rust(&l.path).and_then(|f| rename(f, l, &r)));
            if let (Some(fix), Some(last)) = (fix, out.items.last_mut()) {
                last.fix = Some(fix);
            }
        }
    }
}

/// `Get_Unsent records` -> `get-unsent-records`.
fn kebab(label: &str) -> String {
    let lower = label.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words.join("-")
}

/// Replace the label's string literal, when it is written plainly.
fn rename(file: &RustFile, label: &Label, renamed: &str) -> Option<Fix> {
    let text = file.line(label.line);
    let start = text.char_indices().nth(label.column - 1)?.0;
    let old = format!("\"{}\"", label.text);
    if !text[start..].starts_with(&old) {
        return None; // escapes or a raw string
    }
    let line = format!(
        "{}\"{renamed}\"{}",
        &text[..start],
        &text[start + old.len()..]
    );
    // rustfmt rewraps a call past `max_width`, or one whose arguments grow
    // past `fn_call_width` (60).
    let grows_past = call_args(text, start).is_some_and(|args| {
        args <= 60 && args + renamed.chars().count() - label.text.chars().count() > 60
    });
    if line.chars().count() > 100 || grows_past {
        return None;
    }
    Some(Fix::replace_line(label.line, line))
}

/// The width of the arguments of the call whose parentheses, on this line,
/// hold the byte offset `at`.
fn call_args(text: &str, at: usize) -> Option<usize> {
    let mut depth = 0;
    let open = text[..at].char_indices().rev().find(|&(_, c)| {
        match c {
            ')' => depth += 1,
            '(' if depth == 0 => return true,
            '(' => depth -= 1,
            _ => {}
        }
        false
    })?;
    let mut depth = 0;
    let close = text[open.0 + 1..].char_indices().find(|&(_, c)| {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return true,
            ')' => depth -= 1,
            _ => {}
        }
        false
    })?;
    Some(text[open.0 + 1..open.0 + 1 + close.0].chars().count())
}

fn is_kebab(label: &str) -> bool {
    !label.is_empty()
        && label.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

struct Reports<'p> {
    path: &'p str,
    /// Depth of per-record loops and closures in the current fn (a `return`
    /// resets it: what returns runs once).
    loops: usize,
    labels: Vec<Label>,
    in_loop: Vec<(usize, usize, String)>,
}

impl Reports<'_> {
    fn label(&mut self, expr: &syn::Expr) {
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) = expr
        else {
            return; // computed labels are checked where they are built
        };
        let start = s.span().start();
        self.labels.push(Label {
            path: self.path.to_string(),
            line: start.line,
            column: start.column + 1,
            text: s.value(),
        });
    }

    /// Run `f` with the loop depth reset (a nested fn, or a `return`).
    fn once(&mut self, f: impl FnOnce(&mut Self)) {
        let loops = std::mem::take(&mut self.loops);
        f(self);
        self.loops = loops;
    }
}

/// Whether a loop over `iterable` (binding `pat`) runs once per record, not
/// once per record *type* or collection.
fn per_record(iterable: &syn::Expr, pat: Option<&syn::Pat>) -> bool {
    let mut t = errors::text(iterable).to_lowercase();
    if let Some(pat) = pat {
        t.push(' ');
        t.push_str(&errors::text(pat).to_lowercase());
    }
    !t.contains("type") && PER_RECORD.iter().any(|w| t.contains(w))
}

impl<'ast> Visit<'ast> for Reports<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.once(|v| syn::visit::visit_item_fn(v, f));
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.once(|v| syn::visit::visit_impl_item_fn(v, f));
    }
    fn visit_expr_return(&mut self, e: &'ast syn::ExprReturn) {
        self.once(|v| syn::visit::visit_expr_return(v, e));
    }
    fn visit_expr_for_loop(&mut self, e: &'ast syn::ExprForLoop) {
        self.visit_expr(&e.expr);
        let counts = usize::from(per_record(&e.expr, Some(&e.pat)));
        self.loops += counts;
        self.visit_block(&e.body);
        self.loops -= counts;
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if m.method == "report_error" && m.args.len() == 1 {
            self.label(&m.args[0]);
        }
        self.visit_expr(&m.receiver);
        let per_item =
            PER_ITEM.contains(&m.method.to_string().as_str()) && per_record(&m.receiver, None);
        for arg in &m.args {
            let closure = matches!(arg, syn::Expr::Closure(_));
            if per_item && closure {
                self.loops += 1;
                self.visit_expr(arg);
                self.loops -= 1;
            } else {
                self.visit_expr(arg);
            }
        }
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = syntax::macro_name(mac);
        if name == "report_error" || name == "breadcrumb" {
            if name == "report_error" {
                if let Some(first) = errors::macro_args(mac).and_then(|a| a.into_iter().next()) {
                    self.label(&first);
                }
            }
            if self.loops > 0 {
                let start = mac.path.segments[0].ident.span().start();
                self.in_loop
                    .push((start.line, start.column + 1, format!("{name}!")));
            }
        }
        syn::visit::visit_macro(self, mac);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check, check_unified};

    const PATH: &str = "components/remote_settings/src/error.rs";
    const OTHER: (&str, &str) = ("components/suggest/src/lib.rs", "");

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[OTHER, (PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/error-report-labels/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 6);
        // The label takes the crate's own prefix.
        assert_eq!(
            fixed(&found[0]),
            Some((
                6,
                "                .report_error(\"remote-settings-backoff\") // flagged: copied from the suggest crate".into()
            ))
        );
    }

    /// The line a fix replaces, and its new text.
    fn fixed(f: &crate::Finding) -> Option<(usize, String)> {
        let edit = &f.fix.as_ref()?.edits[0];
        assert_eq!((edit.remove, edit.insert.len()), (1, 1));
        Some((edit.line, edit.insert[0].clone()))
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/error-report-labels/do.rs")).is_empty());
    }

    #[test]
    fn flags_labels_that_are_not_kebab_case() {
        let src = "fn f() {\n    report_error!(\"remote-settings-get_records\", \"x\");\n    report_error!(\"Failed to read the database\", \"x\");\n}\n";
        let found = run(src);
        assert_eq!(found.len(), 2);
        let kebab = "    report_error!(\"remote-settings-get-records\", \"x\");";
        assert_eq!(fixed(&found[0]), Some((2, kebab.into())));
        // A short label needs a name chosen.
        assert_eq!(fixed(&found[1]), None);
    }

    #[test]
    fn flags_reports_per_record_only() {
        let src = "fn f(records: Vec<R>, types: Vec<T>) {\n    for record in records {\n        report_error!(\"remote-settings-bad-record\", \"x\");\n    }\n    for record_type in types {\n        breadcrumb!(\"ingesting {record_type}\");\n    }\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3]);
    }

    #[test]
    fn allows_one_label_in_several_functions() {
        let src = "fn a() { report_error!(\"remote-settings-read\", \"x\"); }\nfn b() { report_error!(\"remote-settings-read\", \"y\"); }\n";
        assert!(run(src).is_empty());
    }

    #[test]
    fn flags_unprefixed_labels_only_when_added() {
        let src = "fn a() {\n    report_error!(\"invalid-record\", \"x\");\n}\n";
        assert!(run(src).is_empty());
        let diff = "--- a/components/remote_settings/src/error.rs\n+++ b/components/remote_settings/src/error.rs\n@@ -2,0 +2 @@\n+    report_error!(\"invalid-record\", \"x\");\n";
        let found = check_unified(&RULE, &[OTHER, (PATH, src)], diff);
        assert_eq!(found.len(), 1);
        let prefixed = "    report_error!(\"remote-settings-invalid-record\", \"x\");";
        assert_eq!(fixed(&found[0]), Some((2, prefixed.into())));
    }
}
