//! Every transaction bound to a variable is committed (drop = silent rollback)
//!
//! Docs and examples: `rules/transaction-must-commit/`.

use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::sql;
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "transaction-must-commit",
    area: Area::Sql,
    title: "Every transaction bound to a variable is committed (drop = silent rollback)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Methods that open a transaction (rusqlite, `sql_support`, places).
const BEGIN: &[&str] = &[
    "transaction",
    "unchecked_transaction",
    "unchecked_transaction_imm",
    "transaction_with_behavior",
    "begin_transaction",
];

/// Methods that end one on purpose.
const END: &[&str] = &["commit", "rollback", "finish"];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) {
            continue;
        }
        let mut reported = BTreeSet::new();
        for (_, block, _) in sql::functions(file) {
            let mut lets = Lets { found: Vec::new() };
            lets.visit_block(block);
            for (name, line) in lets.found {
                let mut uses = Uses {
                    name: &name,
                    after: line,
                    ended: Vec::new(),
                    moved: false,
                    early_ok: Vec::new(),
                    writes: Vec::new(),
                    block_ended: false,
                };
                uses.visit_block(block);
                if uses.moved || uses.writes.is_empty() || !reported.insert(line) {
                    continue;
                }
                let first_write = uses.writes.iter().min().copied().unwrap_or(line);
                match uses.ended.iter().max() {
                    None => out.add(
                        &file.path,
                        line,
                        format!("`{name}` is never committed, so its writes are rolled back when it is dropped; call `{name}.commit()?`"),
                    ),
                    Some(&end) => {
                        for ret in uses.early_ok.iter().filter(|r| **r < end && **r > first_write) {
                            out.add(
                                &file.path,
                                *ret,
                                format!("this `return Ok(..)` skips `{name}.commit()`, so the writes made in `{name}` are rolled back; commit before returning"),
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Statements that change the database.
fn is_write_sql(text: &str) -> bool {
    sql::lex(text).first().is_some_and(|t| {
        [
            "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "DROP", "ALTER",
        ]
        .iter()
        .any(|w| t.is(w))
    })
}

/// Helpers that, by their name, write what they are given.
fn writes_by_name(name: &str) -> bool {
    const VERBS: &[&str] = &[
        "put",
        "set",
        "insert",
        "delete",
        "update",
        "reset",
        "write",
        "remove",
        "clear",
        "store",
        "save",
        "add",
        "apply",
        "wipe",
        "drop",
        "create",
        "upsert",
        "mark",
        "import",
        "migrate",
        "purge",
        "prune",
        "replace",
        "scrub",
        "bump",
        "increment",
        "record",
    ];
    VERBS.iter().any(|v| {
        name == *v
            || name
                .strip_prefix(v)
                .is_some_and(|rest| rest.starts_with('_'))
    })
}

/// `let tx = conn.transaction()?;` and friends: `(name, line)`.
struct Lets {
    found: Vec<(String, usize)>,
}

impl<'ast> Visit<'ast> for Lets {
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Some(name), Some(init)) = (binding(&local.pat), &local.init) {
            if opens_transaction(&init.expr) {
                self.found.push((name, local.let_token.span.start().line));
            }
        }
        syn::visit::visit_local(self, local);
    }
}

fn binding(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(p) => Some(p.ident.to_string()),
        syn::Pat::Type(t) => binding(&t.pat),
        _ => None,
    }
}

fn opens_transaction(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Try(t) => opens_transaction(&t.expr),
        syn::Expr::Paren(p) => opens_transaction(&p.expr),
        syn::Expr::MethodCall(m) => {
            let method = m.method.to_string();
            if matches!(
                method.as_str(),
                "unwrap" | "expect" | "map_err" | "context" | "with_context"
            ) {
                return opens_transaction(&m.receiver);
            }
            BEGIN.contains(&method.as_str())
        }
        _ => false,
    }
}

/// How the transaction variable is used after its `let`.
struct Uses<'n> {
    name: &'n str,
    after: usize,
    /// Lines of `name.commit()`/`rollback()`.
    ended: Vec<usize>,
    /// Passed or returned by value: someone else ends it.
    moved: bool,
    /// Lines of `return Ok(..)` after the `let`, not preceded by an end.
    early_ok: Vec<usize>,
    /// Lines that write: write SQL, or `&tx` handed to a writing helper.
    writes: Vec<usize>,
    /// The current block already ended the transaction.
    block_ended: bool,
}

impl Uses<'_> {
    fn is_name(&self, e: &syn::Expr) -> bool {
        matches!(e, syn::Expr::Path(p) if p.path.is_ident(self.name))
    }

    /// `&tx` or `&*tx`.
    fn is_ref(&self, e: &syn::Expr) -> bool {
        match e {
            syn::Expr::Reference(r) => self.is_name(&r.expr) || self.is_ref(&r.expr),
            syn::Expr::Unary(u) => self.is_name(&u.expr),
            _ => false,
        }
    }

    fn note_helper(&mut self, callee: &str, args: impl Iterator<Item = bool>, line: usize) {
        if line > self.after && writes_by_name(callee) && args.into_iter().any(|a| a) {
            self.writes.push(line);
        }
    }

    fn ends(&self, stmt: &syn::Stmt) -> bool {
        let expr = match stmt {
            syn::Stmt::Expr(e, _) => e,
            _ => return false,
        };
        let expr = match expr {
            syn::Expr::Try(t) => &*t.expr,
            e => e,
        };
        matches!(expr, syn::Expr::MethodCall(m)
            if self.is_name(&m.receiver) && END.contains(&m.method.to_string().as_str()))
    }
}

impl<'ast> Visit<'ast> for Uses<'_> {
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_block(&mut self, b: &'ast syn::Block) {
        let saved = self.block_ended;
        for stmt in &b.stmts {
            self.visit_stmt(stmt);
            if self.ends(stmt) {
                self.block_ended = true;
            }
        }
        self.block_ended = saved;
    }

    fn visit_lit_str(&mut self, s: &'ast syn::LitStr) {
        let line = s.span().start().line;
        if line > self.after && is_write_sql(&s.value()) {
            self.writes.push(line);
        }
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*c.func {
            if let Some(last) = p.path.segments.last() {
                let refs: Vec<bool> = c.args.iter().map(|a| self.is_ref(a)).collect();
                self.note_helper(
                    &last.ident.to_string(),
                    refs.into_iter(),
                    c.span().start().line,
                );
            }
        }
        syn::visit::visit_expr_call(self, c);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let refs: Vec<bool> = m.args.iter().map(|a| self.is_ref(a)).collect();
        self.note_helper(
            &m.method.to_string(),
            refs.into_iter(),
            m.method.span().start().line,
        );
        if self.is_name(&m.receiver) {
            if END.contains(&m.method.to_string().as_str()) {
                self.ended.push(m.method.span().start().line);
            }
            for arg in &m.args {
                self.visit_expr(arg);
            }
            return;
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_expr_reference(&mut self, r: &'ast syn::ExprReference) {
        if self.is_name(&r.expr) {
            return;
        }
        syn::visit::visit_expr_reference(self, r);
    }

    fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
        if self.is_name(&f.base) {
            return;
        }
        syn::visit::visit_expr_field(self, f);
    }

    fn visit_expr_unary(&mut self, u: &'ast syn::ExprUnary) {
        if matches!(u.op, syn::UnOp::Deref(_)) && self.is_name(&u.expr) {
            return;
        }
        syn::visit::visit_expr_unary(self, u);
    }

    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if p.path.is_ident(self.name) && p.span().start().line > self.after {
            self.moved = true;
        }
    }

    fn visit_expr_return(&mut self, r: &'ast syn::ExprReturn) {
        let line = r.span().start().line;
        if line > self.after && !self.block_ended {
            if let Some(syn::Expr::Call(call)) = r.expr.as_deref() {
                if matches!(&*call.func, syn::Expr::Path(p) if p.path.is_ident("Ok")) {
                    self.early_ok.push(line);
                }
            }
        }
        syn::visit::visit_expr_return(self, r);
    }

    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        // A `return` inside a closure leaves the closure, not the function.
        let saved = self.early_ok.len();
        syn::visit::visit_expr_closure(self, c);
        self.early_ok.truncate(saved);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if m.path.span().start().line <= self.after {
            return;
        }
        let parsed = m.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        );
        match parsed {
            Ok(exprs) => {
                for e in &exprs {
                    self.visit_expr(e);
                }
            }
            // Can't see inside: if it names the transaction, assume it is handled.
            Err(_) => {
                if m.tokens
                    .clone()
                    .into_iter()
                    .any(|t| t.to_string() == self.name)
                {
                    self.moved = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/autofill/src/sync/engine.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/transaction-must-commit/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/transaction-must-commit/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_moved_and_rolled_back_transactions() {
        let code = r#"
fn a(conn: &Connection) -> Result<Transaction<'_>> {
    let tx = conn.transaction()?;
    Ok(tx)
}
fn b(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    finish(tx)
}
fn c(conn: &Connection) -> Result<u32> {
    let tx = conn.unchecked_transaction_imm()?;
    tx.execute("DELETE FROM t", [])?;
    tx.rollback()?;
    Ok(1)
}
fn read_only(conn: &Connection) -> Result<u32> {
    let tx = conn.transaction()?;
    let dao = Dao::new(&tx);
    Ok(tx.query_row("SELECT n FROM t", [], |r| r.get(0))? + dao.count()?)
}
fn d(db: &PlacesDb) -> Result<()> {
    let tx = db.begin_transaction()?;
    let items = update_items(&tx)?;
    let store = Store { tx };
    store.apply(items)
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_early_ok_before_commit() {
        let code = r#"
fn a(conn: &Connection, items: &[Item]) -> Result<()> {
    let tx = conn.transaction()?;
    for item in items {
        insert_item(&tx, item)?;
        if item.last {
            return Ok(());
        }
    }
    tx.commit()?;
    Ok(())
}
fn b(conn: &Connection, items: &[Item]) -> Result<()> {
    let tx = conn.transaction()?;
    if items.is_empty() {
        return Ok(()); // nothing written yet
    }
    items.iter().try_for_each(|i| { if i.skip { return Ok(()); } insert_item(&tx, i) })?;
    tx.execute("DELETE FROM t", [])?;
    if items.len() > 3 {
        tx.rollback()?;
        return Ok(());
    }
    tx.commit()?;
    Ok(())
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 7);
    }
}
