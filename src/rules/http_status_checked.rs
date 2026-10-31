//! Check the HTTP status before using a response body; only explicit success is success
//!
//! Docs and examples: `rules/http-status-checked/`.
//!
//! Within one function, a viaduct response (`let r = req.send()?`,
//! `client.send_sync(..)?`) must have its status looked at
//! (`require_success()`, `is_success()`, `r.status`, or a `check*(&r)`
//! helper) before its body is read (`json()`, `text()`, `.body`) or the
//! response is stored (`store*`/`cache*`/`insert*`/`put*`/`write*`/`save*`).
//! Range checks such as `status >= 400` are flagged too: they let 1xx and 3xx
//! through as success.

use std::collections::BTreeMap;

use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "http-status-checked",
    area: Area::Errors,
    title: "Check the HTTP status before using a response body; only explicit success is success",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Calls that send a viaduct request and return the raw response.
const SENDS: &[&str] = &["send", "send_sync", "send_with_policy", "send_ohttp"];

/// Words in a helper's name that say it looks at the status.
const CHECKS: &[&str] = &[
    "check", "success", "status", "error", "ensure", "verify", "require",
];

/// Words in a call's name that say it keeps the response.
const STORES: &[&str] = &[
    "store", "cache", "insert", "put", "write", "save", "persist",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        for f in errors::functions(&file.ast) {
            let mut v = Uses::default();
            v.visit_block(f.block);
            let mut hits = v.direct;
            for (name, events) in &v.events {
                let first_check = events
                    .iter()
                    .filter(|e| e.kind == Event::Check)
                    .map(|e| e.pos)
                    .min();
                let unchecked = events
                    .iter()
                    .filter(|e| e.kind != Event::Check)
                    .filter(|e| first_check.is_none_or(|c| e.pos < c))
                    .min_by_key(|e| e.pos);
                if let Some(e) = unchecked {
                    let what = if e.kind == Event::Store {
                        format!("`{name}` is stored before its HTTP status is checked")
                    } else {
                        format!("the body of `{name}` is used before its HTTP status is checked")
                    };
                    hits.push((e.pos, format!("{what}; call `require_success()?` first")));
                }
            }
            hits.extend(v.ranges);
            for ((line, column), message) in hits {
                if !support.contains(line) {
                    out.add_at(&file.path, line, column, message);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Check,
    Body,
    Store,
}

struct Use {
    kind: Event,
    pos: (usize, usize),
}

#[derive(Default)]
struct Uses {
    /// Response variables and what happens to them.
    events: BTreeMap<String, Vec<Use>>,
    /// `send()?.json()` chains.
    direct: Vec<((usize, usize), String)>,
    /// `status >= 400`-style checks.
    ranges: Vec<((usize, usize), String)>,
}

fn pos<T: syn::spanned::Spanned>(node: &T) -> (usize, usize) {
    let s = node.span().start();
    (s.line, s.column + 1)
}

/// Whether an expression sends a request and yields the unchecked response.
fn is_send(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Try(t) => is_send(&t.expr),
        syn::Expr::Paren(p) => is_send(&p.expr),
        syn::Expr::MethodCall(m) => {
            let name = m.method.to_string();
            SENDS.contains(&name.as_str()) && (name != "send" || m.args.is_empty())
        }
        syn::Expr::Match(m) => is_send(&m.expr),
        _ => false,
    }
}

/// The response variable an expression reads, if it is one we track.
fn var_of(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(p) => p.path.get_ident().map(|i| i.to_string()),
        syn::Expr::Reference(r) => var_of(&r.expr),
        syn::Expr::Paren(p) => var_of(&p.expr),
        _ => None,
    }
}

impl Uses {
    fn record(&mut self, name: &str, kind: Event, pos: (usize, usize)) {
        if let Some(events) = self.events.get_mut(name) {
            events.push(Use { kind, pos });
        }
    }
}

impl<'ast> Visit<'ast> for Uses {
    fn visit_item(&mut self, _: &'ast syn::Item) {
        // Nested items are checked as functions of their own.
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let Some(init) = &l.init {
            self.visit_expr(&init.expr);
            if is_send(&init.expr) {
                let name = match &l.pat {
                    syn::Pat::Ident(i) => Some(i.ident.to_string()),
                    syn::Pat::Type(t) => match &*t.pat {
                        syn::Pat::Ident(i) => Some(i.ident.to_string()),
                        _ => None,
                    },
                    syn::Pat::Tuple(t) => match t.elems.first() {
                        Some(syn::Pat::Ident(i)) => Some(i.ident.to_string()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(name) = name {
                    self.events.insert(name, Vec::new());
                }
            }
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if let Some(var) = var_of(&m.receiver) {
            match name.as_str() {
                "require_success" | "is_success" => self.record(&var, Event::Check, pos(m)),
                "json" | "text" => self.record(&var, Event::Body, pos(&m.method)),
                _ => {}
            }
        }
        if matches!(name.as_str(), "json" | "text") && m.args.is_empty() && is_send(&m.receiver) {
            self.direct.push((
                pos(&m.method),
                format!("`.{name}()` reads the body of a response whose HTTP status was never checked; call `require_success()?` first"),
            ));
        }
        self.calls(&name, &m.args);
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*c.func {
            let name = errors::text(&p.path).to_lowercase();
            self.calls(&name, &c.args);
        }
        syn::visit::visit_expr_call(self, c);
    }
    fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
        if let (Some(var), syn::Member::Named(member)) = (var_of(&f.base), &f.member) {
            if member == "status" {
                self.record(&var, Event::Check, pos(f));
            } else if member == "body" {
                self.record(&var, Event::Body, pos(member));
            }
        }
        syn::visit::visit_expr_field(self, f);
    }
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        use syn::BinOp::{Ge, Gt, Le, Lt};
        if matches!(b.op, Ge(_) | Gt(_) | Le(_) | Lt(_)) {
            let (l, r) = (text(&b.left), text(&b.right));
            let is_status = |s: &str| s == "status" || s.ends_with(".status") || s == "status_code";
            let is_bound = |s: &str| matches!(s, "300" | "399" | "400" | "299");
            if (is_status(&l) && is_bound(&r)) || (is_status(&r) && is_bound(&l)) {
                let op = match b.op {
                    Ge(_) => ">=",
                    Gt(_) => ">",
                    Le(_) => "<=",
                    _ => "<",
                };
                self.ranges.push((
                    pos(b),
                    format!("`{l} {op} {r}` lets 1xx and 3xx responses through as success; use `require_success()`/`is_success()` or match the exact code"),
                ));
            }
        }
        syn::visit::visit_expr_binary(self, b);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        errors::visit_macro_args(self, m);
        syn::visit::visit_macro(self, m);
    }
}

impl Uses {
    /// A response passed to a helper: a status check, or storage.
    fn calls(&mut self, name: &str, args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) {
        let name = name.to_lowercase();
        let last = name.rsplit("::").next().unwrap_or(&name);
        for arg in args {
            let Some(var) = var_of(arg) else { continue };
            if CHECKS.iter().any(|c| last.contains(c)) {
                self.record(&var, Event::Check, pos(arg));
            } else if STORES.iter().any(|s| last.contains(s)) {
                self.record(&var, Event::Store, pos(arg));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/merino/src/client.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/http-status-checked/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/http-status-checked/do.rs")).is_empty());
    }

    #[test]
    fn flags_range_checks_and_chains() {
        let src = "fn f(req: Request) -> Result<T> {\n    let response = req.send()?;\n    if response.status >= 400 {\n        return Err(Error::Http);\n    }\n    Ok(Request::get(u).send()?.json()?)\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 6]);
    }

    #[test]
    fn flags_storing_before_the_check() {
        let src = "fn f(&self, client: &Client) -> Result<Response> {\n    let response = client.send_sync(req)?;\n    self.store.store_with_ttl(&hash, &response, &ttl)?;\n    check_status(&response)?;\n    Ok(response)\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3]);
    }

    #[test]
    fn accepts_checks_and_error_bodies() {
        let src = "fn f(req: Request) -> Result<T> {\n    let resp = req.send()?;\n    if !resp.is_success() {\n        return Err(Error::Status(resp.status, resp.text().to_string()));\n    }\n    Ok(resp.json()?)\n}\nfn g(&self) -> Result<T> {\n    let response = Request::post(url).send()?;\n    self.check_response_error(&response)?;\n    Ok(response.json()?)\n}\n";
        assert!(run(src).is_empty());
    }
}
