//! SQLite footguns: `INSERT OR REPLACE`, `COUNT(*)` for existence, `LIKE` prefix, unsafe `LOWER()`
//!
//! Docs and examples: `rules/sqlite-footguns/`.

use std::collections::{BTreeMap, HashSet};

use syn::visit::Visit;

use crate::helpers::sql::{self, SqlText, Tok, TokKind};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "sqlite-footguns",
    area: Area::Sql,
    title: "SQLite footguns: `INSERT OR REPLACE`, `COUNT(*)` for existence, `LIKE` prefix, unsafe `LOWER()`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let texts = sql::sql_texts(cx.ws);
    // Group by package: foreign keys only matter within one database.
    let mut by_package: BTreeMap<String, Vec<&SqlText>> = BTreeMap::new();
    for text in &texts {
        let key = cx
            .ws
            .package_of(&text.path)
            .map(|p| p.manifest.clone())
            .unwrap_or_default();
        by_package.entry(key).or_default().push(text);
    }
    for texts in by_package.values() {
        let schema = sql::schema_from(texts);
        let fk_enforced = texts.iter().any(|t| enables_foreign_keys(&t.value));
        let deliberate = deliberate_replaces(texts, &schema);
        for text in texts.iter().filter(|t| !t.in_test) {
            let toks = text.tokens();
            if fk_enforced {
                replace_on_parent(text, &toks, &schema, &deliberate, out);
            }
            count_compared_with_zero(text, &toks, out);
            like_prefix(text, &toks, out);
            case_folding(text, &toks, out);
        }
    }
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) {
            continue;
        }
        let mut v = CountUses { hits: Vec::new() };
        v.visit_file(&file.ast);
        for line in v.hits {
            out.add(
                &file.path,
                line,
                "`COUNT(*)` only compared with 0 counts every row; ask `SELECT EXISTS(…)` or use `ConnExt::exists`",
            );
        }
    }
}

fn enables_foreign_keys(sql: &str) -> bool {
    let toks = sql::lex(sql);
    toks.windows(4).any(|w| {
        w[0].is("PRAGMA")
            && w[1].is("foreign_keys")
            && w[2].is_punct("=")
            && (w[3].is("ON") || w[3].is("true") || w[3].text == "1")
    })
}

/// The tables `INSERT OR REPLACE INTO` / `REPLACE INTO` write in `toks`, with
/// the offset of the statement.
fn replaced_tables(toks: &[Tok]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if !t.is("REPLACE") || !toks.get(i + 1).is_some_and(|n| n.is("INTO")) {
            continue;
        }
        let at = if i >= 2 && toks[i - 1].is("OR") && toks[i - 2].is("INSERT") {
            toks[i - 2].start
        } else {
            t.start
        };
        if let Some(table) = table_at(toks, i + 2) {
            out.push((table, at));
        }
    }
    out
}

/// Tables written by any `INSERT … INTO` in `toks`.
fn inserted_tables(toks: &[Tok]) -> Vec<String> {
    toks.iter()
        .enumerate()
        .filter(|(i, t)| t.is("INTO") && *i > 0)
        .filter_map(|(i, _)| table_at(toks, i + 1))
        .collect()
}

/// `name` or `schema.name` at `i`.
fn table_at(toks: &[Tok], i: usize) -> Option<String> {
    if toks.get(i + 1).is_some_and(|d| d.is_punct(".")) {
        return toks.get(i + 2).map(|t| t.name());
    }
    toks.get(i).map(|t| t.name())
}

/// Children that `REPLACE` on `table` wipes (or nulls) through their foreign key.
fn cascading_children<'s>(
    schema: &'s sql::Schema,
    table: &'s str,
) -> impl Iterator<Item = (&'s sql::Table, &'s sql::ForeignKey)> + 's {
    schema.children_of(table).filter(|(_, fk)| {
        matches!(
            fk.on_delete.as_deref(),
            Some("CASCADE") | Some("SET NULL") | Some("SET DEFAULT")
        )
    })
}

/// Tables whose replace-and-cascade is deliberate: somewhere in the crate,
/// the same function (or SQL text) that replaces a row re-inserts its children.
fn deliberate_replaces(texts: &[&SqlText], schema: &sql::Schema) -> HashSet<String> {
    let mut groups: BTreeMap<(String, usize), (Vec<String>, Vec<String>)> = BTreeMap::new();
    for text in texts {
        let key = (
            text.path.clone(),
            text.func
                .as_ref()
                .map(|f| f.start_line)
                .unwrap_or(text.line),
        );
        let toks = text.tokens();
        let group = groups.entry(key).or_default();
        group
            .0
            .extend(replaced_tables(&toks).into_iter().map(|(t, _)| t));
        group.1.extend(inserted_tables(&toks));
    }
    let mut out = HashSet::new();
    for (replaced, inserted) in groups.values() {
        for table in replaced {
            if cascading_children(schema, table)
                .any(|(child, _)| inserted.iter().any(|i| i.eq_ignore_ascii_case(&child.name)))
            {
                out.insert(table.to_ascii_lowercase());
            }
        }
    }
    out
}

/// `INSERT OR REPLACE INTO t` / `REPLACE INTO t`: the conflicting row is
/// deleted first, so `ON DELETE CASCADE`/`SET NULL` children are wiped.
fn replace_on_parent(
    text: &SqlText,
    toks: &[Tok],
    schema: &sql::Schema,
    deliberate: &HashSet<String>,
    out: &mut Findings,
) {
    for (table, at) in replaced_tables(toks) {
        if deliberate.contains(&table.to_ascii_lowercase()) {
            continue;
        }
        if let Some((child, fk)) = cascading_children(schema, &table).next() {
            out.add(
                &text.path,
                text.line_at(at),
                format!(
                    "`REPLACE` deletes the old `{table}` row first, which fires `ON DELETE {}` on `{}`; use `INSERT … ON CONFLICT(…) DO UPDATE`",
                    fk.on_delete.as_deref().unwrap_or_default(),
                    child.name
                ),
            );
        }
    }
}

/// `(SELECT COUNT(*) …) > 0` in SQL.
fn count_compared_with_zero(text: &SqlText, toks: &[Tok], out: &mut Findings) {
    for (i, t) in toks.iter().enumerate() {
        if !(t.is_punct("(")
            && toks.get(i + 1).is_some_and(|s| s.is("SELECT"))
            && is_count(&toks[i + 2..]))
        {
            continue;
        }
        let Some(close) = sql::matching(toks, i) else {
            continue;
        };
        let after = &toks[close + 1..];
        let before = &toks[..i];
        if compared_with_zero(after.first(), after.get(1))
            || compared_with_zero_rev(
                before.last(),
                before.len().checked_sub(2).map(|j| &before[j]),
            )
        {
            out.add(
                &text.path,
                text.line_at(toks[i + 2].start),
                "`COUNT(*)` only compared with 0 counts every row; use `EXISTS (SELECT 1 …)`",
            );
        }
    }
}

/// `COUNT(*)` or `COUNT(1)` as the only selected column, from a real table
/// (`sqlite_master` is tiny).
fn is_count(toks: &[Tok]) -> bool {
    toks.len() >= 6
        && toks[0].is("COUNT")
        && toks[1].is_punct("(")
        && (toks[2].is_punct("*") || toks[2].text == "1")
        && toks[3].is_punct(")")
        && toks[4].is("FROM")
        && !["sqlite_master", "sqlite_schema", "sqlite_temp_master"]
            .iter()
            .any(|t| toks[5].is(t))
}

fn compared_with_zero(op: Option<&Tok>, value: Option<&Tok>) -> bool {
    let (Some(op), Some(value)) = (op, value) else {
        return false;
    };
    let zero = value.kind == TokKind::Num && value.text == "0";
    let one = value.kind == TokKind::Num && value.text == "1";
    (zero && ["=", "==", "!=", "<>", ">"].iter().any(|o| op.is_punct(o)))
        || (one && [">=", "<"].iter().any(|o| op.is_punct(o)))
}

fn compared_with_zero_rev(op: Option<&Tok>, value: Option<&Tok>) -> bool {
    let (Some(op), Some(value)) = (op, value) else {
        return false;
    };
    value.kind == TokKind::Num
        && value.text == "0"
        && ["=", "==", "!=", "<>", "<"].iter().any(|o| op.is_punct(o))
}

/// `LIKE :x || '%'`: `%`/`_` in the input are wildcards, and no index is used.
fn like_prefix(text: &SqlText, toks: &[Tok], out: &mut Findings) {
    for w in toks.windows(4) {
        if w[0].is("LIKE")
            && matches!(w[1].kind, TokKind::Param | TokKind::Hole)
            && w[2].is_punct("||")
            && w[3].text == "'%'"
        {
            out.add(
                &text.path,
                text.line_at(w[0].start),
                format!(
                    "`LIKE {p} || '%'` treats `%` and `_` in the input as wildcards and cannot use an index; use `BETWEEN {p} AND {p} || x'FFFF'`",
                    p = w[1].text
                ),
            );
        }
    }
}

/// `LOWER(col)`/`UPPER(col)` in a condition or ordering: SQLite folds ASCII only.
fn case_folding(text: &SqlText, toks: &[Tok], out: &mut Findings) {
    let mut in_condition = false;
    for (i, t) in toks.iter().enumerate() {
        if t.is("WHERE") || t.is("ON") || t.is("BY") || t.is("HAVING") {
            in_condition = true;
        } else if t.is("SELECT") || t.is("SET") || t.is("VALUES") {
            in_condition = false;
        }
        let call =
            (t.is("LOWER") || t.is("UPPER")) && toks.get(i + 1).is_some_and(|p| p.is_punct("("));
        let arg = toks.get(i + 2);
        if in_condition
            && call
            && arg
                .is_some_and(|a| matches!(a.kind, TokKind::Word | TokKind::Quoted | TokKind::Param))
        {
            out.add(
                &text.path,
                text.line_at(t.start),
                format!(
                    "`{}()` folds only ASCII in SQLite and defeats indexes; normalize the text in Rust or compare with a collation",
                    t.text.to_ascii_uppercase()
                ),
            );
        }
    }
}

/// Rust that runs `SELECT COUNT(*) …` and only compares the result with 0.
struct CountUses {
    hits: Vec<usize>,
}

impl<'ast> Visit<'ast> for CountUses {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        for (i, stmt) in block.stmts.iter().enumerate() {
            let syn::Stmt::Local(local) = stmt else {
                continue;
            };
            let Some(init) = &local.init else { continue };
            let Some(name) = pat_ident(&local.pat) else {
                continue;
            };
            if !is_count_query(&init.expr) {
                continue;
            }
            let mut uses = Uses {
                name: &name,
                compared: 0,
                other: 0,
            };
            for rest in &block.stmts[i + 1..] {
                uses.visit_stmt(rest);
            }
            if uses.compared > 0 && uses.other == 0 {
                self.hits.push(local.let_token.span.start().line);
            }
        }
        syn::visit::visit_block(self, block);
    }

    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        let (query, zero) = if is_zero(&b.right) {
            (&*b.left, true)
        } else if is_zero(&b.left) {
            (&*b.right, true)
        } else {
            (&*b.left, false)
        };
        if zero && is_comparison(&b.op) && is_count_query(query) {
            self.hits
                .push(syn::spanned::Spanned::span(query).start().line);
        }
        syn::visit::visit_expr_binary(self, b);
    }
}

struct Uses<'n> {
    name: &'n str,
    compared: usize,
    other: usize,
}

impl<'ast> Visit<'ast> for Uses<'_> {
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        let side = |e: &syn::Expr| is_var(e, self.name);
        if is_comparison(&b.op)
            && ((side(&b.left) && is_zero(&b.right)) || (side(&b.right) && is_zero(&b.left)))
        {
            self.compared += 1;
            return;
        }
        syn::visit::visit_expr_binary(self, b);
    }

    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if p.path.is_ident(self.name) {
            self.other += 1;
        }
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if m.tokens
            .clone()
            .into_iter()
            .any(|t| t.to_string() == self.name)
        {
            self.other += 1;
        }
    }
}

fn pat_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(p) => Some(p.ident.to_string()),
        syn::Pat::Type(t) => pat_ident(&t.pat),
        _ => None,
    }
}

fn is_var(e: &syn::Expr, name: &str) -> bool {
    match e {
        syn::Expr::Path(p) => p.path.is_ident(name),
        syn::Expr::Paren(p) => is_var(&p.expr, name),
        syn::Expr::Unary(u) => is_var(&u.expr, name),
        _ => false,
    }
}

fn is_zero(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Lit(l) => matches!(&l.lit, syn::Lit::Int(i) if i.base10_digits() == "0"),
        syn::Expr::Paren(p) => is_zero(&p.expr),
        _ => false,
    }
}

fn is_comparison(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::Gt(_) | syn::BinOp::Eq(_) | syn::BinOp::Ne(_) | syn::BinOp::Lt(_)
    )
}

/// `conn.query_row("SELECT COUNT(*) FROM …", …)?` and friends.
fn is_count_query(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Try(t) => is_count_query(&t.expr),
        syn::Expr::Paren(p) => is_count_query(&p.expr),
        syn::Expr::MethodCall(m) => {
            if matches!(
                m.method.to_string().as_str(),
                "unwrap" | "expect" | "unwrap_or" | "unwrap_or_default"
            ) {
                return is_count_query(&m.receiver);
            }
            if !sql::SQL_METHODS.contains(&m.method.to_string().as_str()) {
                return false;
            }
            let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            })) = m.args.first()
            else {
                return false;
            };
            let value = s.value();
            let toks = sql::lex(&value);
            toks.first().is_some_and(|t| t.is("SELECT")) && is_count(&toks[1..])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/db.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/sqlite-footguns/dont.rs"))],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found.iter().any(|f| f.message.contains("LIKE")));
        assert!(found.iter().any(|f| f.message.contains("COUNT")));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/sqlite-footguns/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    const SCHEMA: &str = r#"
const SQL: &str = "
CREATE TABLE suggestions(id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE);
CREATE TABLE keywords(
    keyword TEXT NOT NULL,
    suggestion_id INTEGER NOT NULL REFERENCES suggestions(id) ON DELETE CASCADE
);
CREATE TABLE meta(key TEXT PRIMARY KEY, value NOT NULL);
";
fn prepare(conn: &Connection) { conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap(); }
"#;

    #[test]
    fn flags_replace_on_cascading_parents_only() {
        let code = r#"
fn f(conn: &Connection) {
    conn.execute_cached("INSERT OR REPLACE INTO suggestions(url) VALUES(?)", [u]).unwrap();
    conn.execute_cached("INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)", [k, v]).unwrap();
}
"#;
        let found = check(
            &RULE,
            &[
                ("components/suggest/src/schema.rs", SCHEMA),
                ("components/suggest/src/db.rs", code),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert!(found[0].message.contains("keywords"));
    }

    #[test]
    fn ignores_replace_when_the_crate_rebuilds_the_children() {
        let code = r#"
fn store(conn: &Connection) {
    conn.execute_cached("REPLACE INTO suggestions(url) VALUES(?)", [u]).unwrap();
    conn.execute_cached("INSERT INTO keywords(keyword, suggestion_id) VALUES(?, ?)", [k, i]).unwrap();
}
fn tombstone(conn: &Connection) {
    conn.execute_cached("REPLACE INTO suggestions(url) VALUES(?)", [u]).unwrap();
}
"#;
        let found = check(
            &RULE,
            &[
                ("components/suggest/src/schema.rs", SCHEMA),
                ("components/suggest/src/db.rs", code),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_counts_used_as_numbers() {
        let code = r#"
fn f(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))?;
    if n > 0 { log(n); }
    Ok(n)
}
fn g(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT COUNT(*) FROM t WHERE (SELECT COUNT(*) FROM u) > 2", [], |r| r.get(0))? > 1)
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_count_subquery_and_lower() {
        let code = r#"
fn f(conn: &Connection) {
    conn.execute(
        "DELETE FROM t
         WHERE (SELECT COUNT(*) FROM u WHERE u.t = t.id) = 0",
        [],
    );
    conn.query_row("SELECT id FROM t WHERE LOWER(title) = :q", [], |r| r.get(0));
    conn.query_row("SELECT id FROM t WHERE title LIKE '%' || :q || '%'", [], |r| r.get(0));
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 5);
        assert!(found[1].message.contains("LOWER"));
    }
}
