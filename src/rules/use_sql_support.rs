//! Use `sql_support` helpers (`ConnExt`, cached statements, `ConnectionInitializer`) instead of raw rusqlite
//!
//! Docs and examples: `rules/use-sql-support/`.

use std::collections::BTreeMap;

use syn::visit::Visit;

use crate::helpers::sql::{self, SqlText};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "use-sql-support",
    area: Area::Sql,
    title: "Use `sql_support` helpers (`ConnExt`, cached statements, `ConnectionInitializer`) instead of raw rusqlite",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Methods that run (or prepare) only the first statement of their SQL.
const SINGLE: &[&str] = &[
    "execute",
    "execute_cached",
    "prepare",
    "prepare_cached",
    "query_row",
    "query_row_and_then",
    "query_rows_and_then",
    "query_rows_and_then_cached",
    "try_query_row",
    "exists",
];

fn check(cx: &Context, out: &mut Findings) {
    let mut consts: BTreeMap<(String, String), SqlText> = BTreeMap::new();
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) {
            continue;
        }
        let (texts, uses) = sql::rust_sql(file);
        for text in &texts {
            if let Some(item) = &text.item {
                consts.insert((file.path.clone(), item.clone()), text.clone());
            }
            if text.method.as_deref().is_some_and(|m| SINGLE.contains(&m)) && !text.in_test {
                if let Some(second) = second_statement(text) {
                    out.add(
                        &text.path,
                        text.line_at(second),
                        format!(
                            "`{}` runs only the first statement of this SQL; use `execute_batch` for several statements",
                            text.method.as_deref().unwrap_or_default()
                        ),
                    );
                }
            }
        }
        for u in uses {
            if !SINGLE.contains(&u.method.as_str()) || file.in_test(u.line) {
                continue;
            }
            let Some(text) = consts.get(&(file.path.clone(), u.ident.clone())) else {
                continue;
            };
            if second_statement(text).is_some() {
                out.add(
                    &u.path,
                    u.line,
                    format!(
                        "`{}` runs only the first statement of `{}`; use `execute_batch`",
                        u.method, u.ident
                    ),
                );
            }
        }
        let mut daos = DaoWrites { hits: Vec::new() };
        daos.visit_file(&file.ast);
        for (line, name) in daos.hits {
            out.add(
                &file.path,
                line,
                format!("`{name}` writes to the database but takes `&self`; take `&mut self` so read-only callers cannot write"),
            );
        }
    }
}

/// The offset of a second non-empty statement, if the SQL has one.
fn second_statement(text: &SqlText) -> Option<usize> {
    let stmts = text.statements();
    if stmts.len() < 2
        || text.in_macro
            && stmts
                .iter()
                .any(|s| s.toks.iter().any(|t| t.kind == sql::TokKind::Hole))
    {
        return None;
    }
    Some(stmts[1].start())
}

/// `pub fn` with `&self` on a `*Dao` type whose own SQL writes.
struct DaoWrites {
    hits: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for DaoWrites {
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let is_dao = i.trait_.is_none()
            && syntax::type_ident(&i.self_ty).is_some_and(|t| t.ends_with("Dao"));
        if !is_dao {
            return;
        }
        for item in &i.items {
            let syn::ImplItem::Fn(f) = item else { continue };
            let shared_self = matches!(
                f.sig.receiver(),
                Some(r) if r.reference.is_some() && r.mutability.is_none()
            );
            if !shared_self || !matches!(f.vis, syn::Visibility::Public(_)) {
                continue;
            }
            let mut writes = WriteSql(false);
            writes.visit_block(&f.block);
            if writes.0 {
                self.hits
                    .push((f.sig.ident.span().start().line, f.sig.ident.to_string()));
            }
        }
    }
}

struct WriteSql(bool);

impl<'ast> Visit<'ast> for WriteSql {
    fn visit_lit_str(&mut self, s: &'ast syn::LitStr) {
        let value = s.value();
        let first = sql::lex(&value);
        if first
            .first()
            .is_some_and(|t| ["INSERT", "UPDATE", "DELETE", "REPLACE"].contains(&t.text))
            && sql::looks_like_sql(&value, false)
        {
            self.0 = true;
        }
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if let Ok(exprs) = m.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            for e in &exprs {
                self.visit_expr(e);
            }
        }
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
            &[(PATH, include_str!("../../rules/use-sql-support/dont.rs"))],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found.iter().any(|f| f.message.contains("&mut self")));
        assert!(found.iter().any(|f| f.message.contains("execute_batch")));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/use-sql-support/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_multi_statement_constants() {
        let code = r#"
const SETUP: &str = "CREATE TABLE a(x); CREATE TABLE b(y);";
const ONE: &str = "CREATE TABLE c(x);";
fn upgrade_from(tx: &Transaction) -> Result<()> {
    tx.execute(SETUP, [])?;
    tx.execute(ONE, [])?;
    tx.execute_batch(SETUP)?;
    Ok(())
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 5);
    }

    #[test]
    fn ignores_reads_private_helpers_and_other_types() {
        let code = r#"
impl SuggestDao<'_> {
    pub fn fetch(&self) -> Result<Vec<String>> {
        self.conn.query_rows_and_then_cached("SELECT url FROM t", [], |r| r.get(0))
    }
    fn insert_helper(&self) -> Result<()> {
        self.conn.execute_cached("INSERT INTO t(url) VALUES('x')", [])?;
        Ok(())
    }
}
impl Store {
    pub fn clear(&self) -> Result<()> {
        self.conn.execute_cached("DELETE FROM t", [])?;
        Ok(())
    }
}
"#;
        assert!(check(&RULE, &[(PATH, code)]).is_empty());
    }
}
