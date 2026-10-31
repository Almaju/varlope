//! Never call foreign callbacks, do network I/O, or re-lock while holding a store lock
//!
//! Docs and examples: `rules/no-callbacks-or-io-under-lock/`.
//!
//! Foreign callbacks are the traits Kotlin/Swift/JS implement:
//! `#[uniffi::export(callback_interface)]`/`with_foreign` traits and UDL
//! `callback interface`/`[WithForeign]` interfaces. A field or static typed
//! `dyn Trait` of one of them holds a foreign object. While a guard is live
//! (`let g = x.lock()` until the end of its block or `drop(g)`, or a
//! temporary guard within one expression), the rule flags: a method call on
//! such a field, an HTTP send, a second lock of the same lock, and
//! overwriting a stored callback through the guard (its destructor calls
//! into foreign code).

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::errors::{self, text, GuardFns, LockCall, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::udl::{self, UdlKind};

pub static RULE: Rule = Rule {
    id: "no-callbacks-or-io-under-lock",
    area: Area::Concurrency,
    title: "Never call foreign callbacks, do network I/O, or re-lock while holding a store lock",
    kind: Kind::Code,
    clippy: &[
        "clippy::significant_drop_tightening",
        "clippy::significant_drop_in_scrutinee",
    ],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let foreign = foreign_traits(cx);
    for file in errors::shipped_files(cx.ws) {
        let fields = foreign_fields(&file.ast, &foreign);
        let guard_fns = errors::guard_fns(&file.ast);
        let support = TestSupport::of(file);
        for f in errors::functions(&file.ast) {
            let mut v = UnderLock {
                fields: &fields,
                guard_fns: &guard_fns,
                guards: Vec::new(),
                hits: Vec::new(),
            };
            v.visit_block(f.block);
            for (line, column, message) in v.hits {
                if !support.contains(line) {
                    out.add_at(&file.path, line, column, message);
                }
            }
        }
    }
}

/// Traits implemented on the foreign side, by name.
fn foreign_traits(cx: &Context) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in cx.ws.rust_files() {
        for item in syntax::all_items(&file.ast) {
            if let syn::Item::Trait(t) = item {
                let foreign = syntax::find_attr(&t.attrs, "uniffi::export")
                    .map(syntax::attr_args)
                    .is_some_and(|a| a.contains("callback_interface") || a.contains("with_foreign"));
                if foreign {
                    out.insert(t.ident.to_string());
                }
            }
        }
    }
    for file in cx.ws.files_with_ext(&["udl"]) {
        for item in udl::parse(&file.text) {
            let foreign = item.kind == UdlKind::CallbackInterface
                || (item.kind == UdlKind::Interface && item.has_attr("WithForeign"));
            if foreign {
                out.insert(item.name);
            }
        }
    }
    out
}

/// Fields and statics of a file that hold a foreign object (`Arc<dyn T>`,
/// `RwLock<Option<Box<dyn T>>>`...).
fn foreign_fields(file: &syn::File, foreign: &BTreeSet<String>) -> BTreeSet<String> {
    let holds = |ty: &syn::Type| {
        let t = syntax::type_string(ty);
        foreign.iter().any(|f| t.contains(&format!("dyn {f}")))
    };
    let mut out = BTreeSet::new();
    for item in syntax::all_items(file) {
        match item {
            syn::Item::Struct(s) => {
                for field in &s.fields {
                    if let Some(name) = &field.ident {
                        if holds(&field.ty) {
                            out.insert(format!("self.{name}"));
                        }
                    }
                }
            }
            syn::Item::Static(s) if holds(&s.ty) => {
                out.insert(s.ident.to_string());
            }
            _ => {}
        }
    }
    out
}

struct Guard {
    name: String,
    lock: LockCall,
}

struct UnderLock<'a> {
    fields: &'a BTreeSet<String>,
    guard_fns: &'a GuardFns,
    /// Live guards, innermost last.
    guards: Vec<Guard>,
    hits: Vec<(usize, usize, String)>,
}

impl UnderLock<'_> {
    fn hit<T: syn::spanned::Spanned>(&mut self, node: &T, message: String) {
        let start = node.span().start();
        if !self.hits.iter().any(|(l, c, _)| (*l, *c) == (start.line, start.column + 1)) {
            self.hits.push((start.line, start.column + 1, message));
        }
    }

    /// The foreign field a receiver goes through: `self.telemetry` in
    /// `self.telemetry.record(..)`, or a guard of one.
    fn foreign_receiver(&self, receiver: &syn::Expr) -> Option<String> {
        let t = text(receiver);
        let root = t
            .trim_start_matches('*')
            .split(['.', '('])
            .next()
            .unwrap_or("");
        if let Some(g) = self.guards.iter().find(|g| g.name == root) {
            return self.fields.contains(&g.lock.target).then(|| g.lock.target.clone());
        }
        self.fields
            .iter()
            .find(|f| t == **f || t.starts_with(&format!("{f}.")) || t.starts_with(&format!("{f}(")))
            .cloned()
    }

    fn held(&self) -> Option<&Guard> {
        self.guards.last()
    }
}

impl<'ast> Visit<'ast> for UnderLock<'_> {
    fn visit_block(&mut self, b: &'ast syn::Block) {
        let depth = self.guards.len();
        syn::visit::visit_block(self, b);
        self.guards.truncate(depth);
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        syn::visit::visit_local(self, l);
        let Some(init) = &l.init else { return };
        let syn::Pat::Ident(name) = &l.pat else { return };
        if let Some(lock) = errors::lock_call(&init.expr, self.guard_fns) {
            self.guards.push(Guard {
                name: name.ident.to_string(),
                lock,
            });
        }
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        syn::visit::visit_expr_call(self, c);
        // `drop(guard)` ends it.
        if let (syn::Expr::Path(p), Some(arg)) = (&*c.func, c.args.first()) {
            if p.path.is_ident("drop") {
                let name = text(arg);
                self.guards.retain(|g| g.name != name);
            }
        }
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        // A temporary guard: `self.callback.read().on_change(..)`.
        if let Some(lock) = errors::lock_call(&m.receiver, self.guard_fns) {
            if self.fields.contains(&lock.target) {
                self.hit(
                    &m.method,
                    format!("`{}` calls into foreign code while holding the lock on `{}`; clone the callback out of the guard first", m.method, lock.target),
                );
            }
        }
        if let Some(held) = self.held() {
            let held_key = held.lock.key.clone();
            let held_name = held.name.clone();
            let held_shared = held.lock.shared;
            if let Some(field) = self.foreign_receiver(&m.receiver) {
                self.hit(
                    &m.method,
                    format!("`{field}.{}` calls into foreign code while `{held_name}` is locked; drop the guard first", m.method),
                );
            } else if errors::is_network_call(m) {
                self.hit(
                    &m.method,
                    format!("`.{}()` does network I/O while `{held_name}` is locked; drop the guard first", m.method),
                );
            } else if let Some(again) =
                errors::lock_call(&syn::Expr::MethodCall(m.clone()), self.guard_fns)
            {
                let both_shared = held_shared && again.shared;
                if again.key == held_key && !both_shared {
                    self.hit(
                        &m.method,
                        format!("`{}` is locked again while `{held_name}` still holds it; this deadlocks, reuse `{held_name}`", again.key),
                    );
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_assign(&mut self, a: &'ast syn::ExprAssign) {
        // `*self.callback.write() = ..` / `*guard = None`: the old callback
        // is dropped, calling into foreign code, under the lock.
        if let syn::Expr::Unary(u) = &*a.left {
            if matches!(u.op, syn::UnOp::Deref(_)) {
                let target = errors::lock_call(&u.expr, self.guard_fns)
                    .map(|l| l.target)
                    .or_else(|| {
                        let name = text(&u.expr);
                        self.guards
                            .iter()
                            .find(|g| g.name == name)
                            .map(|g| g.lock.target.clone())
                    });
                if let Some(target) = target.filter(|t| self.fields.contains(t)) {
                    self.hit(
                        &*a.left,
                        format!("replacing the callback in `{target}` drops the old one, a foreign call, under its lock; `take()`/`replace()` it into a local that drops after the guard"),
                    );
                }
            }
        }
        syn::visit::visit_expr_assign(self, a);
    }
    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        // `move` closures usually run later, on another thread.
        if c.capture.is_none() {
            syn::visit::visit_expr_closure(self, c);
        }
    }
    fn visit_item(&mut self, _: &'ast syn::Item) {
        // Nested items are visited as functions of their own.
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/client.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/no-callbacks-or-io-under-lock/dont.rs"));
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![10, 11]);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/no-callbacks-or-io-under-lock/do.rs")).is_empty());
    }

    const HEADER: &str = "#[uniffi::export(callback_interface)]\npub trait Cb: Send + Sync { fn changed(&self); }\npub struct S { db: Mutex<Db>, cb: RwLock<Box<dyn Cb>> }\n";

    #[test]
    fn flags_relocking_and_replacing_callbacks() {
        let src = format!("{HEADER}impl S {{\n    fn a(&self) {{\n        let db = self.db.lock();\n        let again = self.db.lock();\n    }}\n    fn b(&self) {{\n        *self.cb.write() = Box::new(Noop);\n        self.cb.read().changed();\n    }}\n}}\n");
        let lines: Vec<usize> = run(&src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![7, 10, 11]);
    }

    #[test]
    fn ignores_dropped_and_scoped_guards() {
        let src = format!("{HEADER}impl S {{\n    fn a(&self, cb: &dyn Cb) {{\n        let db = self.db.lock();\n        drop(db);\n        let again = self.db.lock();\n    }}\n    fn b(&self) {{\n        {{\n            let db = self.db.lock();\n        }}\n        Request::get(u).send();\n    }}\n}}\n");
        assert!(run(&src).is_empty());
    }
}
