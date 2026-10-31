//! Acquire a lock once per operation, at the layer that owns the state
//!
//! Docs and examples: `rules/lock-once-per-operation/`.
//!
//! A review hint: points at functions that take the same lock twice in a row
//! (another thread can change the state between the two), and at locks taken
//! inside a loop. An explicit `drop(guard)` before the second lock, or a
//! comment at it saying why the release is needed (a callback, network I/O),
//! marks the release as deliberate.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::errors::{self, GuardFns, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "lock-once-per-operation",
    area: Area::Concurrency,
    title: "Acquire a lock once per operation, at the layer that owns the state",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let guard_fns = errors::guard_fns(&file.ast);
        let support = TestSupport::of(file);
        for f in errors::functions(&file.ast) {
            let mut v = Locks {
                guard_fns: &guard_fns,
                loops: 0,
                seen: BTreeMap::new(),
                guards: BTreeMap::new(),
                dropped: BTreeSet::new(),
                hits: Vec::new(),
            };
            v.visit_block(f.block);
            for (line, column, message) in v.hits {
                if support.contains(line) || errors::comment_explains_relock(file, line) {
                    continue;
                }
                out.add_at(&file.path, line, column, message);
            }
        }
    }
}

/// Iterator adapters whose closure runs once per item.
const PER_ITEM: &[&str] = &["map", "for_each", "filter_map", "flat_map", "filter", "try_for_each"];

struct Locks<'a> {
    guard_fns: &'a GuardFns,
    /// Depth of loops and per-item closures.
    loops: usize,
    /// Lock key -> line of its first lock in this function.
    seen: BTreeMap<String, usize>,
    /// Guard variable -> lock key.
    guards: BTreeMap<String, String>,
    /// Locks released on purpose with `drop(guard)`.
    dropped: BTreeSet<String>,
    hits: Vec<(usize, usize, String)>,
}

impl<'ast> Visit<'ast> for Locks<'_> {
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.visit_expr(&m.receiver);
        let per_item = PER_ITEM.contains(&m.method.to_string().as_str());
        for arg in &m.args {
            let closure = matches!(arg, syn::Expr::Closure(_));
            self.loops += usize::from(per_item && closure);
            self.visit_expr(arg);
            self.loops -= usize::from(per_item && closure);
        }
        if matches!(m.method.to_string().as_str(), "unwrap" | "expect") {
            return; // the lock call inside was already seen
        }
        let Some(lock) = errors::lock_call(&syn::Expr::MethodCall(m.clone()), self.guard_fns)
        else {
            return;
        };
        let start = m.method.span().start();
        if self.loops > 0 {
            self.hits.push((
                start.line,
                start.column + 1,
                format!("`{}` is locked once per iteration; look at taking it once around the loop", lock.key),
            ));
        } else if let Some(first) = self.seen.get(&lock.key) {
            if *first != start.line && !self.dropped.contains(&lock.key) {
                self.hits.push((
                    start.line,
                    start.column + 1,
                    format!("`{}` was already locked on line {first}; the state can change in between, so look at doing the whole operation under one lock", lock.key),
                ));
            }
        } else {
            self.seen.insert(lock.key, start.line);
        }
    }
    fn visit_local(&mut self, l: &'ast syn::Local) {
        syn::visit::visit_local(self, l);
        if let (syn::Pat::Ident(name), Some(init)) = (&l.pat, &l.init) {
            if let Some(lock) = errors::lock_call(&init.expr, self.guard_fns) {
                self.guards.insert(name.ident.to_string(), lock.key);
            }
        }
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        syn::visit::visit_expr_call(self, c);
        if let (syn::Expr::Path(p), Some(arg)) = (&*c.func, c.args.first()) {
            if p.path.is_ident("drop") {
                if let Some(key) = self.guards.get(&errors::text(arg)) {
                    self.dropped.insert(key.clone());
                }
            }
        }
    }
    fn visit_expr_for_loop(&mut self, e: &'ast syn::ExprForLoop) {
        self.visit_expr(&e.expr);
        self.loops += 1;
        self.visit_block(&e.body);
        self.loops -= 1;
    }
    fn visit_expr_while(&mut self, e: &'ast syn::ExprWhile) {
        self.loops += 1;
        syn::visit::visit_expr_while(self, e);
        self.loops -= 1;
    }
    fn visit_expr_loop(&mut self, e: &'ast syn::ExprLoop) {
        self.loops += 1;
        syn::visit::visit_expr_loop(self, e);
        self.loops -= 1;
    }
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/stateful/nimbus_client.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/lock-once-per-operation/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/lock-once-per-operation/do.rs")).is_empty());
    }

    #[test]
    fn flags_check_then_act_across_two_locks() {
        let src = "fn make_client(&self) -> Client {\n    let config = self.state.lock().config.clone();\n    let client = Client::new(config);\n    self.state.lock().clients.push(client.clone());\n    client\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4]);
    }

    #[test]
    fn accepts_deliberate_releases() {
        let src = "fn get(&self) -> Result<()> {\n    let inner = self.inner.lock();\n    drop(inner);\n    self.sync()?;\n    let inner = self.inner.lock();\n    Ok(())\n}\nfn set(&self) {\n    let v = self.x.lock().clone();\n    // the callback may lock `x` itself, so it runs unlocked\n    self.x.lock().push(v);\n}\n";
        assert!(run(src).is_empty());
    }

    #[test]
    fn ignores_one_lock_chain() {
        assert!(run("fn f(&self) { self.state.lock().unwrap().insert(1); }").is_empty());
    }
}
