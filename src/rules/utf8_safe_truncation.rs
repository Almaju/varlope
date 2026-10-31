//! Never truncate or slice strings at byte offsets
//!
//! Docs and examples: `rules/utf8-safe-truncation/`.
//!
//! Without types, a string is a parameter or `let` typed `str`/`String`, a
//! `let` built by a string constructor (`String::new()`, `format!`,
//! `.to_string()`...), or a field declared `String` in the same file. The rule
//! flags `.truncate(n)` on one, and slicing one at a byte limit or literal
//! offset, unless the function steps back to a char boundary
//! (`is_char_boundary`) or takes the offset from the string itself (`find`,
//! `char_indices`, a `starts_with` check).

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "utf8-safe-truncation",
    area: Area::Errors,
    title: "Never truncate or slice strings at byte offsets",
    kind: Kind::Code,
    clippy: &["clippy::string_slice", "clippy::disallowed_methods"],
    in_tests: false,
    check,
};

/// Calls that make an offset a char boundary of the string they run on.
const BOUNDARY_SOURCES: &[&str] = &[
    "find",
    "rfind",
    "char_indices",
    "match_indices",
    "rmatch_indices",
    "is_char_boundary",
    "floor_char_boundary",
    "ceil_char_boundary",
    "len_utf8",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        let fields = string_fields(&file.ast);
        for f in errors::functions(&file.ast) {
            let mut strings: BTreeSet<String> = BTreeSet::new();
            for input in &f.sig.inputs {
                if let syn::FnArg::Typed(t) = input {
                    if is_string_type(&t.ty) {
                        strings.insert(text(&t.pat).trim_start_matches("mut ").to_string());
                    }
                }
            }
            let mut v = Body {
                strings,
                fields: &fields,
                lets: BTreeMap::new(),
                checked: BTreeSet::new(),
                calls: BTreeSet::new(),
                guards: Vec::new(),
                hits: Vec::new(),
                prefix_checks: BTreeMap::new(),
            };
            v.visit_block(f.block);
            for hit in &v.hits {
                if support.contains(hit.line) || v.is_safe(hit) {
                    continue;
                }
                let message = match &hit.what {
                    What::Truncate(n) => format!(
                        "`{}.truncate({n})` panics when byte {n} is inside a multi-byte char; cut at a char boundary instead",
                        hit.string
                    ),
                    What::Slice(n) => format!(
                        "slicing `{}` at byte offset `{n}` panics inside a multi-byte char; use a char-boundary-safe helper or `.get(..)`",
                        hit.string
                    ),
                };
                out.add_at(&file.path, hit.line, hit.column, message);
                let check = v.prefix_checks.get(&(hit.line, hit.column));
                if let (Some(fix), Some(last)) = (
                    check.and_then(|c| starts_with_fix(file, c)),
                    out.items.last_mut(),
                ) {
                    last.fix = Some(fix);
                }
            }
        }
    }
}

/// Field names declared with a string type in a file (and never with another type).
fn string_fields(file: &syn::File) -> BTreeSet<String> {
    let mut strings = BTreeSet::new();
    let mut others = BTreeSet::new();
    for item in syntax::all_items(file) {
        if let syn::Item::Struct(s) = item {
            for field in &s.fields {
                if let Some(name) = &field.ident {
                    if is_string_type(&field.ty) {
                        strings.insert(name.to_string());
                    } else {
                        others.insert(name.to_string());
                    }
                }
            }
        }
    }
    strings.difference(&others).cloned().collect()
}

fn is_string_type(ty: &syn::Type) -> bool {
    let t = syntax::type_string(ty);
    let t = t.trim_start_matches('&').trim_start_matches("mut ");
    let t = t.split_once('\'').map_or(t, |(_, rest)| {
        rest.split_once(' ').map_or(rest, |(_, ty)| ty)
    });
    matches!(t, "str" | "String" | "Cow<str>" | "Cow<'_,str>")
}

/// Whether an initializer builds a `String`.
fn makes_string(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Macro(m) => syntax::macro_name(&m.mac) == "format",
        syn::Expr::Call(c) => {
            let t = text(&c.func);
            t.starts_with("String::")
        }
        syn::Expr::MethodCall(m) => matches!(
            m.method.to_string().as_str(),
            "to_string" | "to_lowercase" | "to_uppercase" | "into_string"
        ),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        }) => true,
        _ => false,
    }
}

/// `&s[..8] != "default/"`: a prefix test written as a slice, which
/// `s.starts_with("default/")` does without panicking.
struct PrefixCheck {
    /// The whole comparison.
    comparison: proc_macro2::Span,
    /// The string sliced, and the literal.
    string: proc_macro2::Span,
    literal: proc_macro2::Span,
    negated: bool,
}

/// The comparison rewritten with `starts_with`, when it is on one line.
fn starts_with_fix(file: &RustFile, check: &PrefixCheck) -> Option<Fix> {
    let range = file.range(check.comparison);
    let (start, end) = (check.comparison.start(), check.comparison.end());
    if start.line != end.line {
        return None;
    }
    let text = file.line(start.line);
    let line_start = file.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let (from, to) = (range.start - line_start, range.end - line_start);
    let not = if check.negated { "!" } else { "" };
    let rewritten = format!(
        "{}{not}{}.starts_with({}){}",
        text.get(..from)?,
        file.text[file.range(check.string)].trim(),
        &file.text[file.range(check.literal)],
        text.get(to..)?
    );
    (rewritten.chars().count() <= 100).then(|| Fix::replace_line(start.line, rewritten))
}

enum What {
    Truncate(String),
    Slice(String),
}

struct Hit {
    line: usize,
    column: usize,
    string: String,
    /// The offset expressions.
    offsets: Vec<syn::Expr>,
    what: What,
}

struct Body<'f> {
    /// Names known to hold a string.
    strings: BTreeSet<String>,
    fields: &'f BTreeSet<String>,
    /// `let` name -> initializer, as text and as an expression.
    lets: BTreeMap<String, (String, syn::Expr)>,
    /// Strings whose offsets were checked with `s.get(..)`.
    checked: BTreeSet<String>,
    /// Every method name called in the body.
    calls: BTreeSet<String>,
    /// Conditions and scrutinees, as text.
    guards: Vec<String>,
    hits: Vec<Hit>,
    /// Slices compared with a literal of their length, by the position of
    /// the slice's range (where its hit is).
    prefix_checks: BTreeMap<(usize, usize), PrefixCheck>,
}

impl Body<'_> {
    fn is_string(&self, expr: &syn::Expr) -> Option<String> {
        let expr = peel(expr);
        let t = text(expr);
        if self.strings.contains(&t) {
            return Some(t);
        }
        if let syn::Expr::Field(f) = expr {
            if let syn::Member::Named(name) = &f.member {
                if self.fields.contains(&name.to_string()) {
                    return Some(t);
                }
            }
        }
        None
    }

    fn is_safe(&self, hit: &Hit) -> bool {
        if self
            .calls
            .iter()
            .any(|c| matches!(c.as_str(), "is_char_boundary" | "floor_char_boundary"))
        {
            return true;
        }
        hit.offsets
            .iter()
            .all(|o| self.offset_is_boundary(&hit.string, o))
    }

    /// Whether an offset into `string` is safe, or at least not a byte limit:
    /// only literal offsets and offsets built from constants (`MAX_TITLE_BYTES`,
    /// `len().min(50)`) are suspects; computed offsets usually come from
    /// walking the string itself.
    fn offset_is_boundary(&self, string: &str, offset: &syn::Expr) -> bool {
        if self.checked.contains(string) {
            return true; // `s.get(..)` already checked these offsets
        }
        let t = text(offset);
        if t == "0" || t == format!("{string}.len()") {
            return true;
        }
        let from_boundary_source = |s: &str| {
            BOUNDARY_SOURCES
                .iter()
                .any(|b| s.contains(&format!(".{b}(")) || s.contains(&format!("{b}(")))
        };
        if from_boundary_source(&t) {
            return true;
        }
        let mut limit = is_byte_limit(offset);
        for ident in idents_in(offset) {
            if let Some(init) = self.lets.get(&ident) {
                if from_boundary_source(&init.0) {
                    return true;
                }
                limit |= is_byte_limit(&init.1);
            }
        }
        if !limit {
            return true;
        }
        // A literal offset after `s.starts_with("http://")` is the prefix length.
        errors::is_literal(offset)
            && self.guards.iter().any(|g| {
                g.contains(string)
                    && ["starts_with", "strip_prefix", "is_ascii", "ends_with"]
                        .iter()
                        .any(|c| g.contains(c))
            })
    }
}

/// An offset made of a literal number or a constant: a byte limit or a fixed
/// prefix length, not a position found in the string.
fn is_byte_limit(expr: &syn::Expr) -> bool {
    let mut found = false;
    errors::scan_tokens(
        quote::ToTokens::to_token_stream(expr),
        &mut |tokens, i| match &tokens[i] {
            proc_macro2::TokenTree::Literal(l) => {
                let l = l.to_string();
                if l.chars().all(|c| c.is_ascii_digit() || c == '_') && l != "0" && l != "1" {
                    found = true;
                }
            }
            proc_macro2::TokenTree::Ident(id) => {
                let id = id.to_string();
                if id.len() > 1
                    && id
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    found = true;
                }
            }
            _ => {}
        },
    );
    found
}

fn peel(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Reference(r) => peel(&r.expr),
        syn::Expr::Paren(p) => peel(&p.expr),
        syn::Expr::Group(g) => peel(&g.expr),
        _ => expr,
    }
}

fn idents_in(expr: &syn::Expr) -> Vec<String> {
    struct V(Vec<String>);
    impl<'a> Visit<'a> for V {
        fn visit_expr_path(&mut self, p: &'a syn::ExprPath) {
            if let Some(i) = p.path.get_ident() {
                self.0.push(i.to_string());
            }
        }
    }
    let mut v = V(Vec::new());
    v.visit_expr(expr);
    v.0
}

impl<'ast> Visit<'ast> for Body<'_> {
    fn visit_item(&mut self, _: &'ast syn::Item) {
        // Nested items are checked as functions of their own.
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        let (pat, ty) = match &l.pat {
            syn::Pat::Type(t) => (&*t.pat, Some(&*t.ty)),
            p => (p, None),
        };
        if let syn::Pat::Ident(ident) = pat {
            let name = ident.ident.to_string();
            let typed = ty.is_some_and(is_string_type);
            let built = l.init.as_ref().is_some_and(|i| {
                makes_string(&i.expr)
                    || matches!(&*i.expr, syn::Expr::MethodCall(m)
                        if m.method == "clone" && self.is_string(&m.receiver).is_some())
            });
            if typed || built {
                self.strings.insert(name.clone());
            }
            if let Some(init) = &l.init {
                self.lets
                    .insert(name, (text(&init.expr), (*init.expr).clone()));
            }
        }
        if let Some(init) = &l.init {
            if init.diverge.is_some() {
                self.guards.push(text(&init.expr));
            }
        }
        syn::visit::visit_local(self, l);
    }
    fn visit_expr_if(&mut self, e: &'ast syn::ExprIf) {
        self.guards.push(text(&e.cond));
        syn::visit::visit_expr_if(self, e);
    }
    fn visit_expr_while(&mut self, e: &'ast syn::ExprWhile) {
        self.guards.push(text(&e.cond));
        syn::visit::visit_expr_while(self, e);
    }
    fn visit_expr_match(&mut self, e: &'ast syn::ExprMatch) {
        self.guards.push(text(&e.expr));
        syn::visit::visit_expr_match(self, e);
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.calls.insert(m.method.to_string());
        if m.method == "get" {
            self.checked.insert(text(peel(&m.receiver)));
        }
        if m.method == "truncate" && m.args.len() == 1 {
            if let Some(string) = self.is_string(&m.receiver) {
                let start = m.method.span().start();
                self.hits.push(Hit {
                    line: start.line,
                    column: start.column + 1,
                    string,
                    offsets: vec![m.args[0].clone()],
                    what: What::Truncate(text(&m.args[0])),
                });
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        let negated = match b.op {
            syn::BinOp::Eq(_) => false,
            syn::BinOp::Ne(_) => true,
            _ => return syn::visit::visit_expr_binary(self, b),
        };
        let pair = match (peel(&b.left), peel(&b.right)) {
            (syn::Expr::Index(i), syn::Expr::Lit(l)) | (syn::Expr::Lit(l), syn::Expr::Index(i)) => {
                Some((i, l))
            }
            _ => None,
        };
        if let Some((
            index,
            syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            },
        )) = pair
        {
            // `s[..N]`, `N` the literal's length, on a string named plainly.
            let prefix = match &*index.index {
                syn::Expr::Range(syn::ExprRange {
                    start: None,
                    limits: syn::RangeLimits::HalfOpen(_),
                    end: Some(end),
                    ..
                }) => match &**end {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Int(n),
                        ..
                    }) => n.base10_parse::<usize>().ok() == Some(lit.value().len()),
                    _ => false,
                },
                _ => false,
            };
            let plain = matches!(
                &*index.expr,
                syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::MethodCall(_)
            );
            if prefix && plain && self.is_string(&index.expr).is_some() {
                let start = syn::spanned::Spanned::span(&index.index).start();
                self.prefix_checks.insert(
                    (start.line, start.column + 1),
                    PrefixCheck {
                        comparison: syn::spanned::Spanned::span(b),
                        string: syn::spanned::Spanned::span(&index.expr),
                        literal: lit.span(),
                        negated,
                    },
                );
            }
        }
        syn::visit::visit_expr_binary(self, b);
    }
    fn visit_expr_index(&mut self, e: &'ast syn::ExprIndex) {
        if let (Some(string), syn::Expr::Range(range)) = (self.is_string(&e.expr), &*e.index) {
            let offsets: Vec<syn::Expr> = [&range.start, &range.end]
                .into_iter()
                .flatten()
                .map(|b| (**b).clone())
                .collect();
            let start = syn::spanned::Spanned::span(&e.index).start();
            let shown = offsets.iter().map(text).collect::<Vec<_>>().join("`/`");
            self.hits.push(Hit {
                line: start.line,
                column: start.column + 1,
                string,
                offsets,
                what: What::Slice(shown),
            });
        }
        syn::visit::visit_expr_index(self, e);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        errors::visit_macro_args(self, m);
        syn::visit::visit_macro(self, m);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/tabs/src/storage.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/utf8-safe-truncation/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 9);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/utf8-safe-truncation/do.rs")).is_empty());
    }

    #[test]
    fn flags_slices_at_byte_limits() {
        let src = "fn f(spec: &str) -> &str {\n    let n = spec.len().min(MAX_BYTES);\n    &spec[..n]\n}\nfn g(name: String) -> bool {\n    &name[..8] == \"default/\"\n}\n";
        let found = run(src);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 6]);
        // Where to cut `spec` is the reader's call; a prefix test is not.
        assert!(found[0].fix.is_none());
        let edit = &found[1].fix.as_ref().expect("a fix").edits[0];
        assert_eq!((edit.line, edit.remove), (6, 1));
        assert_eq!(edit.insert, vec!["    name.starts_with(\"default/\")"]);
    }

    #[test]
    fn fixes_prefix_tests_only() {
        let src = "fn g(name: String) {\n    if &name[..8] != \"default/\" || \"ab\" == &name[..3] { }\n}\n";
        let found = run(src);
        assert_eq!(found.len(), 2, "{found:#?}");
        let edit = &found[0].fix.as_ref().expect("a fix").edits[0];
        let fixed = "    if !name.starts_with(\"default/\") || \"ab\" == &name[..3] { }";
        assert_eq!(edit.insert, vec![fixed]);
        // `"ab"` is 2 bytes: the slice of 3 never equals it.
        assert!(found[1].fix.is_none());
    }

    #[test]
    fn ignores_offsets_found_in_the_string() {
        let src = "fn f(url: &str) -> &str {\n    match url.find(':') {\n        Some(n) => &url[..n],\n        None => url,\n    }\n}\nfn g(s: &str) -> &str {\n    if s.starts_with(\"http://\") { &s[7..] } else { s }\n}\n";
        assert!(run(src).is_empty());
    }

    #[test]
    fn ignores_char_boundary_loops_and_vecs() {
        let src = "fn fit(mut value: String) -> String {\n    let mut end = MAX;\n    while !value.is_char_boundary(end) { end -= 1; }\n    value.truncate(end);\n    value\n}\nfn g(mut list: Vec<u8>) { list.truncate(MAX); }\n";
        assert!(run(src).is_empty());
    }
}
