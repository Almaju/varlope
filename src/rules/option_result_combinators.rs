//! Use Option/Result combinators and `let-else` instead of manual matches
//!
//! Docs and examples: `rules/option-result-combinators/`.

use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "option-result-combinators",
    area: Area::Idioms,
    title: "Use Option/Result combinators and `let-else` instead of manual matches",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::manual_let_else",
        "clippy::option_if_let_else",
        "clippy::map_unwrap_or",
        "clippy::unnecessary_wraps",
        "clippy::match_same_arms",
        "clippy::if_not_else",
        "clippy::redundant_closure_for_method_calls",
    ],
    in_tests: false,
    check,
};

/// Clippy covers most of this rule. varlope adds the case it has no lint
/// for: a `match` on a `Result` with an arm that hands one side back
/// unchanged, which is `.map_err(..)`, `.map(..)` or `?`.
fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        let mut v = Matches(Vec::new());
        v.visit_file(&file.ast);
        for (m, message) in v.0 {
            let (line, column) = file.pos(&m.match_token);
            out.add_at(&file.path, line, column, message);
        }
    }
}

struct Matches<'a>(Vec<(&'a syn::ExprMatch, String)>);

impl<'a> Visit<'a> for Matches<'a> {
    fn visit_expr_match(&mut self, m: &'a syn::ExprMatch) {
        if let Some(message) = forwarding_match(m) {
            self.0.push((m, message));
        }
        syn::visit::visit_expr_match(self, m);
    }
}

/// `match r { Ok(v) => Ok(v), Err(e) => Err(f(e)) }` and friends.
fn forwarding_match(m: &syn::ExprMatch) -> Option<String> {
    let [a, b] = m.arms.as_slice() else {
        return None;
    };
    let (ok, err) = match (variant_binding(&a.pat)?, variant_binding(&b.pat)?) {
        (("Ok", ok), ("Err", err)) => ((ok, a), (err, b)),
        (("Err", err), ("Ok", ok)) => ((ok, b), (err, a)),
        _ => return None,
    };
    if a.guard.is_some() || b.guard.is_some() {
        return None;
    }
    let ok_identity = wraps_binding(&ok.1.body, "Ok", ok.0);
    let err_identity = wraps_binding(&err.1.body, "Err", err.0);
    let err_wraps = wrapped(&err.1.body, "Err").is_some_and(simple);
    let ok_wraps = wrapped(&ok.1.body, "Ok").is_some_and(simple);
    match (ok_identity, err_identity) {
        (true, true) => None, // a no-op match; clippy's `match_like_matches_macro` & co.
        (true, false) if err_wraps => Some(format!(
            "this `match` only converts the error; write `.map_err(|{}| ..)`",
            err.0
        )),
        (false, true) if ok_wraps => Some(format!(
            "this `match` only maps the success value; write `.map(|{}| ..)`",
            ok.0
        )),
        (false, true) if simple(&ok.1.body) => Some(format!(
            "`Err({0}) => Err({0})` just forwards the error; use `?` or `.and_then(..)`",
            err.0
        )),
        _ => None,
    }
}

/// `Ok(v)` / `Err(e)` patterns binding a plain identifier.
fn variant_binding(pat: &syn::Pat) -> Option<(&'static str, &syn::Ident)> {
    let syn::Pat::TupleStruct(ts) = pat else {
        return None;
    };
    let name = ts.path.segments.last()?.ident.to_string();
    let variant = match name.as_str() {
        "Ok" => "Ok",
        "Err" => "Err",
        _ => return None,
    };
    if ts.path.segments.len() > 2 || ts.elems.len() != 1 {
        return None;
    }
    match ts.elems.first()? {
        syn::Pat::Ident(p) if p.subpat.is_none() && p.by_ref.is_none() => Some((variant, &p.ident)),
        _ => None,
    }
}

/// The argument of `Ok(..)`/`Err(..)` when `expr` is exactly that call.
fn wrapped<'e>(expr: &'e syn::Expr, variant: &str) -> Option<&'e syn::Expr> {
    let syn::Expr::Call(call) = strip(expr) else {
        return None;
    };
    let syn::Expr::Path(p) = &*call.func else {
        return None;
    };
    (p.path.segments.last()?.ident == variant && call.args.len() == 1).then(|| &call.args[0])
}

fn wraps_binding(expr: &syn::Expr, variant: &str, binding: &syn::Ident) -> bool {
    wrapped(expr, variant)
        .is_some_and(|arg| matches!(strip(arg), syn::Expr::Path(p) if p.path.is_ident(binding)))
}

/// `{ expr }` -> `expr`.
fn strip(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Block(b) if b.block.stmts.len() == 1 => match &b.block.stmts[0] {
            syn::Stmt::Expr(e, None) => strip(e),
            _ => expr,
        },
        syn::Expr::Paren(p) => strip(&p.expr),
        _ => expr,
    }
}

/// An expression a closure can hold as is: no statements, early returns,
/// `?`, or nested matches (those read better as they are).
fn simple(expr: &syn::Expr) -> bool {
    struct V(bool);
    impl<'a> Visit<'a> for V {
        fn visit_expr(&mut self, e: &'a syn::Expr) {
            match e {
                syn::Expr::Return(_)
                | syn::Expr::Try(_)
                | syn::Expr::Match(_)
                | syn::Expr::If(_)
                | syn::Expr::Loop(_)
                | syn::Expr::ForLoop(_)
                | syn::Expr::While(_)
                | syn::Expr::Break(_)
                | syn::Expr::Continue(_)
                | syn::Expr::Await(_) => self.0 = false,
                syn::Expr::Block(b) if b.block.stmts.len() > 1 => self.0 = false,
                syn::Expr::Macro(m) if syntax::macro_name(&m.mac) != "format" => self.0 = false,
                _ => syn::visit::visit_expr(self, e),
            }
        }
        fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {}
    }
    let mut v = V(true);
    v.visit_expr(strip(expr));
    v.0
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/places/src/api.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/option-result-combinators/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert!(found[0].message.contains("`.map_err(|e| ..)`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/option-result-combinators/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_forwarded_errors_and_mapped_values() {
        let src = "fn a() -> Result<u8> { match get() { Ok(v) => Ok(v + 1), Err(e) => Err(e) } }\n\
                   fn b() -> Result<u8> { match get() { Err(e) => Err(e), Ok(v) => parse(v) } }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("`.map(|v| ..)`"));
        assert!(found[1].message.contains("use `?`"));
    }

    #[test]
    fn accepts_matches_that_do_more() {
        let src = "fn a() -> Result<u8> {\n\
            match get() {\n\
                Ok(v) => Ok(v),\n\
                Err(e) => { warn!(\"retrying\"); retry() }\n\
            }\n\
        }\n\
        fn b() -> Result<u8> { match get() { Ok(v) => Ok(v), Err(Error::Busy) => retry(), Err(e) => Err(e) } }\n\
        fn c() -> Result<u8> { match get() { Ok(v) => Ok(v), Err(e) if e.is_busy() => Err(wrap(e)), Err(e) => Err(e) } }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
