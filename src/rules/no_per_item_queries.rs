//! No per-item queries/requests in loops; long loops check for interruption
//!
//! Docs and examples: `rules/no-per-item-queries/`.

use syn::visit::Visit;

use crate::helpers::sql;
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-per-item-queries",
    area: Area::Sql,
    title: "No per-item queries/requests in loops; long loops check for interruption",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Calls that parse and prepare their SQL again on every call.
const UNCACHED: &[&str] = &[
    "execute",
    "execute_batch",
    "execute_one",
    "prepare",
    "query_row",
    "query_row_and_then",
    "query_rows_and_then",
    "query_rows_into",
    "query_one",
    "conn_ext_query_one",
    "exists",
];

/// Iterator adaptors whose closure runs once per item.
const PER_ITEM: &[&str] = &[
    "for_each",
    "try_for_each",
    "map",
    "filter_map",
    "flat_map",
    "and_then",
    "filter",
    "try_fold",
    "fold",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) {
            continue;
        }
        for (func, block, _) in sql::functions(file) {
            // Migrations run once per database.
            if sql::is_migration_fn(&func.name) {
                continue;
            }
            let mut fn_scan = Mentions::new("err_if_interrupted");
            fn_scan.visit_block(block);
            let mut v = Loops {
                depth: 0,
                interruptible: fn_scan.found,
                write_stmts: Vec::new(),
                lists: Vec::new(),
                prepared: Vec::new(),
                requests: Vec::new(),
                item_loops: 0,
                writes_unchecked: Vec::new(),
                body_writes: false,
                body_checks: false,
            };
            v.visit_block(block);
            for line in v.prepared {
                out.add(
                    &file.path,
                    line,
                    "this statement is prepared again on every iteration; use the `_cached` variant or prepare it once before the loop",
                );
            }
            for line in v.requests {
                out.add(
                    &file.path,
                    line,
                    "this sends one HTTP request per item; fetch the items in one request (or a few batched ones)",
                );
            }
            for line in v.writes_unchecked {
                out.add(
                    &file.path,
                    line,
                    "this loop writes to the database without `err_if_interrupted()`, unlike the rest of the function; check it on every iteration",
                );
            }
        }
    }
}

struct Loops {
    depth: usize,
    /// The function checks for interruption somewhere.
    interruptible: bool,
    /// Statements prepared from write SQL before a loop: `let mut stmt = ….prepare_cached("INSERT …")`.
    write_stmts: Vec<String>,
    /// Local constant lists: `let tables = ["a", "b"];`.
    lists: Vec<String>,
    prepared: Vec<usize>,
    /// `viaduct::Request` sends.
    requests: Vec<usize>,
    /// Depth of loops over a collection (not retries: `loop`, `while`, `0..n`).
    item_loops: usize,
    writes_unchecked: Vec<usize>,
    /// The current outermost loop body writes / checks for interruption.
    body_writes: bool,
    body_checks: bool,
}

impl Loops {
    /// Walk a body that runs once per item. `is_loop` marks `for`/`while`/
    /// `loop` bodies, which also get the interruption check.
    fn body<F: FnOnce(&mut Self)>(&mut self, line: usize, visit: F, is_loop: bool) {
        let outer = self.depth == 0 && is_loop;
        let saved = (self.body_writes, self.body_checks);
        if outer {
            self.body_writes = false;
            self.body_checks = false;
        }
        self.depth += 1;
        visit(self);
        self.depth -= 1;
        if outer {
            if self.interruptible && self.body_writes && !self.body_checks {
                self.writes_unchecked.push(line);
            }
            (self.body_writes, self.body_checks) = saved;
        }
    }
}

/// Whether a subtree mentions an identifier (method or path).
struct Mentions {
    name: &'static str,
    found: bool,
}

impl Mentions {
    fn new(name: &'static str) -> Self {
        Self { name, found: false }
    }
}

impl<'ast> Visit<'ast> for Mentions {
    fn visit_ident(&mut self, i: &'ast proc_macro2::Ident) {
        if i == self.name {
            self.found = true;
        }
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if m.tokens.to_string().contains(self.name) {
            self.found = true;
        }
    }
}

/// `for x in [..]`, `for t in TABLES`, `for t in tables` (a local array):
/// a short, constant list.
fn constant_list(e: &syn::Expr, locals: &[String]) -> bool {
    match e {
        syn::Expr::Array(_) => true,
        syn::Expr::Reference(r) => constant_list(&r.expr, locals),
        syn::Expr::Path(p) => p.path.segments.last().is_some_and(|s| {
            let name = s.ident.to_string();
            locals.contains(&name)
                || name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        }),
        syn::Expr::MethodCall(m) if m.method == "iter" || m.method == "into_iter" => {
            constant_list(&m.receiver, locals)
        }
        _ => false,
    }
}

/// Whether a string literal is SQL that writes.
fn is_write(value: &str) -> bool {
    sql::lex(value).first().is_some_and(|t| {
        ["INSERT", "UPDATE", "DELETE", "REPLACE"]
            .iter()
            .any(|w| t.is(w))
    })
}

impl<'ast> Visit<'ast> for Loops {
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (syn::Pat::Ident(p), Some(init)) = (&local.pat, &local.init) {
            if prepares_write(&init.expr) {
                self.write_stmts.push(p.ident.to_string());
            }
            if constant_list(&init.expr, &[]) {
                self.lists.push(p.ident.to_string());
            }
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_for_loop(&mut self, f: &'ast syn::ExprForLoop) {
        self.visit_expr(&f.expr);
        if constant_list(&f.expr, &self.lists) {
            return;
        }
        let items = !matches!(&*f.expr, syn::Expr::Range(_));
        self.item_loops += usize::from(items);
        self.body(
            f.for_token.span.start().line,
            |v| v.visit_block(&f.body),
            true,
        );
        self.item_loops -= usize::from(items);
    }

    fn visit_expr_while(&mut self, w: &'ast syn::ExprWhile) {
        self.visit_expr(&w.cond);
        self.body(
            w.while_token.span.start().line,
            |v| v.visit_block(&w.body),
            true,
        );
    }

    fn visit_expr_loop(&mut self, l: &'ast syn::ExprLoop) {
        self.body(
            l.loop_token.span.start().line,
            |v| v.visit_block(&l.body),
            true,
        );
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let method = m.method.to_string();
        if self.depth > 0 {
            self.body_checks |= method == "err_if_interrupted";
            let on_write_stmt = matches!(&*m.receiver, syn::Expr::Path(p)
                if p.path.get_ident().is_some_and(|i| self.write_stmts.iter().any(|s| i == s)));
            self.body_writes |= on_write_stmt;
            if let Some(value) = m.args.first().and_then(sql_arg) {
                self.body_writes |= is_write(&value);
                if UNCACHED.contains(&method.as_str()) {
                    self.prepared.push(m.method.span().start().line);
                }
            }
            if self.item_loops > 0
                && method == "send"
                && m.args.is_empty()
                && is_request(&m.receiver)
            {
                self.requests.push(m.method.span().start().line);
            }
        }
        self.visit_expr(&m.receiver);
        let per_item =
            PER_ITEM.contains(&method.as_str()) && !constant_list(&m.receiver, &self.lists);
        for arg in &m.args {
            if per_item && matches!(arg, syn::Expr::Closure(_)) {
                let line = m.method.span().start().line;
                self.item_loops += 1;
                self.body(line, |v| v.visit_expr(arg), false);
                self.item_loops -= 1;
            } else {
                self.visit_expr(arg);
            }
        }
    }
}

/// A viaduct request: `Request::get(url)…`, or a `req`/`request` variable.
fn is_request(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::MethodCall(m) => is_request(&m.receiver),
        syn::Expr::Call(c) => {
            matches!(&*c.func, syn::Expr::Path(p) if p.path.segments.iter().any(|s| s.ident == "Request"))
        }
        syn::Expr::Path(p) => p
            .path
            .get_ident()
            .is_some_and(|i| i == "req" || i == "request"),
        syn::Expr::Paren(p) => is_request(&p.expr),
        _ => false,
    }
}

/// `conn.prepare_cached("INSERT …")?` and friends.
fn prepares_write(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Try(t) => prepares_write(&t.expr),
        syn::Expr::MethodCall(m) if m.method.to_string().starts_with("prepare") => m
            .args
            .first()
            .and_then(sql_arg)
            .is_some_and(|v| is_write(&v)),
        syn::Expr::MethodCall(m) => prepares_write(&m.receiver),
        _ => false,
    }
}

/// The SQL of a call's first argument, when it is a literal or `&format!(..)`.
fn sql_arg(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => {
            let v = s.value();
            sql::looks_like_sql(&v, true).then_some(v)
        }
        syn::Expr::Reference(r) => sql_arg(&r.expr),
        syn::Expr::Paren(p) => sql_arg(&p.expr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/yelp.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/no-per-item-queries/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
    }

    #[test]
    fn ignores_constant_lists_cached_calls_and_migrations() {
        let code = r#"
pub fn clear_database(db: &Connection) -> Result<()> {
    let tables = ["geonames", "keywords_i18n"];
    for t in tables {
        if db.exists("SELECT 1 FROM sqlite_master WHERE name = ?", [t])? {
            db.execute(&format!("DELETE FROM {t}"), ())?;
        }
    }
    for t in &["a", "b"] {
        db.execute("DELETE FROM meta WHERE key = ?", [t])?;
    }
    for url in urls {
        db.execute_cached("INSERT INTO t(url) VALUES(?)", [url])?;
    }
    Ok(())
}
fn upgrade_from_v2(db: &Connection) -> Result<()> {
    for row in rows {
        db.execute("UPDATE t SET name = ? WHERE guid = ?", row)?;
    }
    Ok(())
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_closures_and_unchecked_writes() {
        let code = r#"
fn insert_all(&self, urls: &[String]) -> Result<()> {
    urls.iter().try_for_each(|u| {
        self.conn.execute("INSERT INTO t(url) VALUES(?)", [u]).map(|_| ())
    })?;
    let mut stmt = self.conn.prepare_cached("INSERT INTO k(keyword) VALUES(?)")?;
    for k in keywords {
        stmt.execute([k])?;
    }
    self.scope.err_if_interrupted()?;
    Ok(())
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 4);
        assert!(found[1].message.contains("err_if_interrupted"));
    }

    #[test]
    fn flags_requests_in_loops() {
        let code = r#"
fn fetch_all(&self, ids: &[String]) -> Result<Vec<Response>> {
    let mut out = Vec::new();
    for id in ids {
        out.push(Request::get(self.url(id)?).send()?);
    }
    let (tx, rx) = channel();
    for id in ids {
        tx.send(id.clone())?;
    }
    for attempt in 0..3 {
        if let Ok(r) = Request::get(self.url("x")?).send() { return Ok(vec![r]); }
    }
    Ok(out)
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("HTTP"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/no-per-item-queries/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
