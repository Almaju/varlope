//! No panicking indexing/slicing or unchecked arithmetic on external sizes and timestamps
//!
//! Docs and examples: `rules/no-panicking-index-or-arith/`.
//!
//! clippy's `indexing_slicing` and cast lints do most of the work, for modules
//! that opt in. The native check adds the pattern behind the Yelp crash in
//! Nightly: slicing one value with a bound computed from *another* value's
//! length (`&keyword[query.len()..]`), which panics as soon as the second is
//! longer than the first.

use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-panicking-index-or-arith",
    area: Area::Errors,
    title: "No panicking indexing/slicing or unchecked arithmetic on external sizes and timestamps",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::indexing_slicing",
        "clippy::cast_possible_truncation",
        "clippy::cast_sign_loss",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        for f in errors::functions(&file.ast) {
            let mut v = Slices {
                hits: Vec::new(),
                guards: Vec::new(),
                lets: Vec::new(),
            };
            v.visit_block(f.block);
            for (line, col, sliced, other) in v.hits {
                // The bound was checked, or the sliced value was built from it.
                let guarded = v.guards.iter().any(|g| g.contains(&other))
                    || v.lets
                        .iter()
                        .any(|(name, init)| *name == sliced && init.contains(&other));
                if support.contains(line) || guarded {
                    continue;
                }
                out.add_at(
                    &file.path,
                    line,
                    col,
                    format!(
                        "`{sliced}` is sliced at `{other}.len()`, which panics when `{other}` is longer; use `.get(..)` or `strip_prefix`"
                    ),
                );
            }
        }
    }
}

struct Slices {
    /// `(line, column, sliced value, other value)`.
    hits: Vec<(usize, usize, String, String)>,
    /// Text of conditions, scrutinees and assertions that may bound the lengths.
    guards: Vec<String>,
    /// `(name, initializer)` of each `let`.
    lets: Vec<(String, String)>,
}

impl<'ast> Visit<'ast> for Slices {
    fn visit_item(&mut self, _: &'ast syn::Item) {
        // Nested items are checked as functions of their own.
    }
    fn visit_expr_index(&mut self, e: &'ast syn::ExprIndex) {
        if matches!(&*e.index, syn::Expr::Range(_)) {
            let sliced = base(&e.expr);
            for other in len_receivers(&e.index) {
                // `self.data[..self.len()]`: one value's own invariant.
                if other != sliced && !sliced.starts_with(&format!("{other}.")) {
                    let start = syn::spanned::Spanned::span(&e.index).start();
                    self.hits
                        .push((start.line, start.column + 1, sliced.clone(), other));
                }
            }
        }
        syn::visit::visit_expr_index(self, e);
    }
    fn visit_expr_if(&mut self, e: &'ast syn::ExprIf) {
        self.guards.push(text(&e.cond));
        syn::visit::visit_expr_if(self, e);
    }
    fn visit_expr_match(&mut self, e: &'ast syn::ExprMatch) {
        // `match a.find(&b) { Some(i) => &a[i + b.len()..] }`
        self.guards.push(text(&e.expr));
        syn::visit::visit_expr_match(self, e);
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let Some(init) = &l.init {
            let init = text(&init.expr);
            if l.init.as_ref().is_some_and(|i| i.diverge.is_some()) {
                // `let Some(x) = a.strip_prefix(b) else { .. }`
                self.guards.push(init.clone());
            }
            self.lets.push((pat_name(&l.pat), init));
        }
        syn::visit::visit_local(self, l);
    }
    fn visit_expr_while(&mut self, e: &'ast syn::ExprWhile) {
        self.guards.push(text(&e.cond));
        syn::visit::visit_expr_while(self, e);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let name = crate::syntax::macro_name(m);
        if name.contains("assert") {
            self.guards.push(m.tokens.to_string().replace(' ', ""));
        }
        errors::visit_macro_args(self, m);
        syn::visit::visit_macro(self, m);
    }
}

/// The name a simple `let` pattern binds (`x`, `mut x`, `x: T`).
fn pat_name(pat: &syn::Pat) -> String {
    match pat {
        syn::Pat::Ident(i) => i.ident.to_string(),
        syn::Pat::Type(t) => pat_name(&t.pat),
        _ => String::new(),
    }
}

/// The value being sliced, without `&`, `.as_bytes()`, `.as_str()`.
fn base(expr: &syn::Expr) -> String {
    match expr {
        syn::Expr::Reference(r) => base(&r.expr),
        syn::Expr::Paren(p) => base(&p.expr),
        syn::Expr::MethodCall(m)
            if m.args.is_empty()
                && matches!(
                    m.method.to_string().as_str(),
                    "as_bytes" | "as_str" | "as_slice"
                ) =>
        {
            base(&m.receiver)
        }
        _ => text(expr),
    }
}

/// Every `x` in `x.len()` (or `x.as_bytes().len()`) inside `expr`.
fn len_receivers(expr: &syn::Expr) -> Vec<String> {
    struct V(Vec<String>);
    impl<'a> Visit<'a> for V {
        fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
            if m.method == "len" && m.args.is_empty() {
                self.0.push(base(&m.receiver));
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }
    let mut v = V(Vec::new());
    v.visit_expr(expr);
    v.0
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/yelp.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!(
            "../../rules/no-panicking-index-or-arith/dont.rs"
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!(
            "../../rules/no-panicking-index-or-arith/do.rs"
        ))
        .is_empty());
    }

    #[test]
    fn ignores_slicing_by_own_length() {
        assert!(run("fn f(s: &str) -> &[u8] { &s.as_bytes()[..s.len().min(64)] }").is_empty());
        assert!(run("impl G { fn f(&self) -> &[u8] { &self.data[0..self.len()] } }").is_empty());
    }

    #[test]
    fn ignores_checked_bounds() {
        let found_index = "fn f(href: &str, part: &str) -> &str {\n    match href.find(part) {\n        Some(i) => &href[i + part.len()..],\n        None => href,\n    }\n}\n";
        assert!(run(found_index).is_empty());
        let checked = "fn f(a: &str, b: &str) -> &str {\n    if a.len() >= b.len() {\n        return &a[b.len()..];\n    }\n    a\n}\n";
        assert!(run(checked).is_empty());
        let sized = "fn f(out: &mut [u8]) {\n    let mut derived = expand(out.len());\n    out.swap_with_slice(&mut derived[0..out.len()]);\n}\n";
        assert!(run(sized).is_empty());
    }
}
