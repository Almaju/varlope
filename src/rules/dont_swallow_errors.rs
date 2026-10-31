//! Don't silently discard errors (`let _ =`, `.ok()`, `if let Ok`, `_ => None`)
//!
//! Docs and examples: `rules/dont-swallow-errors/`.
//!
//! Flags the shapes that turn a failure into "nothing": `.ok().flatten()`,
//! `filter_map(Result::ok)`, a statement `x.ok();`, `let _ = fallible();`,
//! `if let Ok(..)` without `else`, and `Err(_) => None`. It stays quiet when
//! the statement logs or reports the error, when a comment right there says
//! why dropping it is fine, and for conversions (`parse`, `try_from`,
//! `Url::parse`...) whose error only means "not that kind of value".

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "dont-swallow-errors",
    area: Area::Errors,
    title: "Don't silently discard errors (`let _ =`, `.ok()`, `if let Ok`, `_ => None`)",
    kind: Kind::Code,
    clippy: &["clippy::let_underscore_must_use", "clippy::map_err_ignore"],
    in_tests: false,
    check,
};

/// Calls whose error just means "the input is not that kind of value".
const CONVERSIONS: &[&str] = &[
    "parse",
    "try_from",
    "try_into",
    "from_str",
    "from_utf8",
    "to_file_path",
    "duration_since",
    "path_segments_mut",
    "canonicalize",
    "read_dir",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        let mut v = Swallows {
            file,
            hits: Vec::new(),
            logged: Vec::new(),
            in_drop: false,
        };
        v.visit_file(&file.ast);
        for (line, col, msg) in v.hits {
            if !support.contains(line) {
                out.add_at(&file.path, line, col, msg);
            }
        }
    }
}

struct Swallows<'f> {
    file: &'f RustFile,
    hits: Vec<(usize, usize, String)>,
    /// For each enclosing statement: whether it logs or reports something.
    logged: Vec<bool>,
    /// Inside `impl Drop`, where there is no one to return an error to.
    in_drop: bool,
}

impl Swallows<'_> {
    fn hit<T: Spanned>(&mut self, at: &T, whole: &dyn Spanned, msg: &str) {
        if self.logged.iter().any(|l| *l) || self.in_drop {
            return;
        }
        let (start, end) = (whole.span().start().line, whole.span().end().line);
        if errors::comment_explains_error(self.file, start, end) {
            return;
        }
        let pos = at.span().start();
        self.hits.push((pos.line, pos.column + 1, msg.to_string()));
    }
}

/// Whether an expression is a conversion whose error is expected.
fn is_conversion(expr: &syn::Expr) -> bool {
    let t = text(expr);
    CONVERSIONS
        .iter()
        .any(|c| t.contains(&format!("{c}(")) || t.contains(&format!("{c}::<")))
}

/// Values that are not an error worth handling: `?` and `.await` results,
/// plain values, channel sends, `OnceCell::set` and map updates.
fn is_benign(expr: &syn::Expr) -> bool {
    let call = match expr {
        syn::Expr::MethodCall(m) => Some(m.method.to_string()),
        syn::Expr::Call(c) => match &*c.func {
            syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        },
        _ => None,
    };
    matches!(
        call.as_deref(),
        None | Some("send" | "try_send" | "set" | "insert" | "remove" | "take" | "replace")
    )
}

impl<'ast> Visit<'ast> for Swallows<'_> {
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let drop = i
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| syntax::path_ends_with(path, "Drop"));
        let outer = std::mem::replace(&mut self.in_drop, drop);
        syn::visit::visit_item_impl(self, i);
        self.in_drop = outer;
    }
    fn visit_stmt(&mut self, s: &'ast syn::Stmt) {
        self.logged.push(errors::logs_something(s));
        if let syn::Stmt::Expr(syn::Expr::MethodCall(m), Some(_)) = s {
            if m.method == "ok"
                && m.args.is_empty()
                && !is_conversion(&m.receiver)
                && !is_benign(&m.receiver)
            {
                self.hit(
                    &m.method,
                    s,
                    "`.ok();` discards this error; propagate it with `?` or log it",
                );
            }
        }
        syn::visit::visit_stmt(self, s);
        self.logged.pop();
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if m.method == "filter_map" && m.args.len() == 1 {
            let arg = &m.args[0];
            let drops_errors = match arg {
                syn::Expr::Path(p) => syntax::path_ends_with(&p.path, "Result::ok"),
                syn::Expr::Closure(c) => matches!(&*c.body, syn::Expr::MethodCall(ok)
                    if ok.method == "ok" && ok.args.is_empty() && !is_conversion(&ok.receiver)),
                _ => false,
            };
            if drops_errors && !is_conversion(&m.receiver) {
                self.hit(
                    &m.method,
                    m,
                    "`filter_map(..ok())` skips every `Err` silently; collect into a `Result` with `?`, or log what is skipped",
                );
            }
        }
        if m.method == "flatten" {
            if let syn::Expr::MethodCall(inner) = &*m.receiver {
                if inner.method == "ok" && inner.args.is_empty() && !is_conversion(&inner.receiver)
                {
                    self.hit(
                        &inner.method,
                        m,
                        "`.ok().flatten()` turns errors into `None`; propagate them with `?` or log them",
                    );
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_if(&mut self, e: &'ast syn::ExprIf) {
        if let syn::Expr::Let(l) = &*e.cond {
            let ok_pattern = matches!(&*l.pat, syn::Pat::TupleStruct(t)
                if t.path.is_ident("Ok"));
            if ok_pattern && e.else_branch.is_none() && !is_conversion(&l.expr) {
                self.hit(
                    &l.pat,
                    e,
                    "`if let Ok(..)` without `else` loses the error; propagate it with `?` or log it in an `else`",
                );
            }
        }
        syn::visit::visit_expr_if(self, e);
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let (syn::Pat::Wild(_), Some(init)) = (&l.pat, &l.init) {
            if !is_benign(&init.expr) && !is_conversion(&init.expr) {
                self.hit(
                    &l.pat,
                    l,
                    "`let _ =` discards this `Result`; propagate it with `?` or log the error",
                );
            }
        }
        syn::visit::visit_local(self, l);
    }
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        if !is_conversion(&m.expr) {
            for arm in &m.arms {
                let err_wild = matches!(&arm.pat, syn::Pat::TupleStruct(t)
                    if t.path.is_ident("Err") && matches!(t.elems.first(), Some(syn::Pat::Wild(_))));
                let body = text(&arm.body);
                let nothing = matches!(
                    body.as_str(),
                    "None"
                        | "Ok(None)"
                        | "{}"
                        | "()"
                        | "continue"
                        | "return"
                        | "return None"
                        | "return Ok(None)"
                        | "return Ok(())"
                        | "{continue}"
                        | "{return}"
                );
                if err_wild && nothing && arm.attrs.is_empty() && !errors::logs_something(&arm.body)
                {
                    self.hit(
                        &arm.pat,
                        arm,
                        &format!("`Err(_) => {body}` drops the error; propagate it or log it"),
                    );
                }
            }
        }
        syn::visit::visit_expr_match(self, m);
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

    const PATH: &str = "components/suggest/src/rs.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/dont-swallow-errors/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/dont-swallow-errors/do.rs")).is_empty());
    }

    #[test]
    fn flags_each_shape() {
        let src = "fn f(&self) -> Option<u64> {
    self.store.cleanup().ok();
    let _ = self.client.sync();
    if let Ok(row) = self.conn.query_one(SQL) {
        use_it(row);
    }
    match self.conn.query_one(SQL) {
        Ok(v) => Some(v),
        Err(_) => None,
    };
    self.store.get_meta(KEY).ok().flatten()
}
";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 3, 4, 9, 11]);
    }

    #[test]
    fn ignores_logged_and_explained_errors() {
        let src = "fn f(&self) {
    if let Err(e) = self.client.sync() { warn!(\"sync failed: {e}\"); }
    self.store.cleanup().inspect_err(|e| warn!(\"cleanup: {e}\")).ok();
    // Best effort: the DB is closed at shutdown anyway.
    let _ = self.db.close();
}
";
        assert!(run(src).is_empty());
    }

    #[test]
    fn ignores_conversions_and_benign_drops() {
        let src = "fn f(s: &str, tx: Sender<u8>) -> Option<Url> {
    let _ = tx.send(1);
    let _ = self.lock()?;
    if let Ok(n) = s.parse::<u32>() { use_it(n); }
    LOGGER.set(logger).ok();
    Url::parse(s).ok()
}
impl Drop for Conn {
    fn drop(&mut self) { let _ = self.conn.close(); }
}
";
        assert!(run(src).is_empty());
    }
}
