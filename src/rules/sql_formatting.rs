//! Format SQL embedded in Rust strings consistently
//!
//! Docs and examples: `rules/sql-formatting/`.

use crate::helpers::sql::{self, SqlText, Tok, TokKind};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "sql-formatting",
    area: Area::Sql,
    title: "Format SQL embedded in Rust strings consistently",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Keywords whose case is checked: lowercase ones are unmistakably keywords
/// (a lowercase `end` or `key` could be a column).
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "UPDATE", "DELETE", "JOIN", "VALUES", "ORDER",
    "GROUP", "HAVING", "LIMIT", "UNION",
];

fn check(cx: &Context, out: &mut Findings) {
    for text in sql::sql_texts(cx.ws) {
        if text.in_test || text.from_file || text.method.is_none() || text.in_migration() {
            continue;
        }
        for stmt in text.statements() {
            if stmt.ddl().is_some() || !is_dml(&stmt.toks) {
                continue;
            }
            if let Some(t) = lowercase_keyword(&stmt.toks) {
                out.add(
                    &text.path,
                    text.line_at(t.start),
                    format!(
                        "SQL keyword `{}` is lowercase; write keywords in uppercase (`{}`)",
                        t.text,
                        t.text.to_ascii_uppercase()
                    ),
                );
                continue;
            }
            if let Some(t) = crowded_line(&text, &stmt.toks) {
                out.add(
                    &text.path,
                    text.line_at(t.start),
                    format!(
                        "`{}` shares a line with `FROM` while the other clauses each have their own line; start it on a new line",
                        t.text
                    ),
                );
            }
        }
    }
}

fn is_dml(toks: &[Tok]) -> bool {
    toks.first().is_some_and(|t| {
        ["SELECT", "INSERT", "UPDATE", "DELETE", "WITH", "REPLACE"]
            .iter()
            .any(|k| t.is(k))
    })
}

/// The first keyword written in lowercase.
fn lowercase_keyword<'a>(toks: &[Tok<'a>]) -> Option<Tok<'a>> {
    toks.iter().copied().find(|t| {
        t.kind == TokKind::Word
            && KEYWORDS.contains(&t.text.to_ascii_uppercase().as_str())
            && t.text.chars().all(|c| c.is_ascii_lowercase())
    })
}

/// A `WHERE` on the same line as its `FROM` (or `JOIN`), in a statement that
/// otherwise puts each clause on its own line (`SELECT` and `FROM` on
/// different lines). Compact one-line statements are fine.
fn crowded_line<'a>(text: &SqlText, toks: &[Tok<'a>]) -> Option<Tok<'a>> {
    let mut depth = 0i32;
    let mut select_line = None;
    let mut from_line = None;
    for t in toks {
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        }
        if depth != 0 || t.kind != TokKind::Word {
            continue;
        }
        let line = text.line_at(t.start);
        if t.is("SELECT") && select_line.is_none() {
            select_line = Some(line);
        } else if t.is("FROM") || t.is("JOIN") {
            from_line = Some(line);
        } else if t.is("WHERE") {
            let split = select_line.is_some() && select_line != from_line;
            if split && from_line == Some(line) {
                return Some(*t);
            }
        } else if t.is("UNION") {
            select_line = None;
        }
    }
    None
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
            &[(PATH, include_str!("../../rules/sql-formatting/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 6);
    }

    #[test]
    fn flags_crowded_where_only_in_clause_per_line_queries() {
        let code = r#"
fn f(conn: &Connection) {
    conn.query_row(
        "SELECT placement_id, ad_body
         FROM ads WHERE placement_id = ?1",
        [], |r| r.get(0));
    conn.query_row("SELECT guid FROM loginsL WHERE is_deleted = 0
                    UNION ALL
                    SELECT guid FROM loginsM WHERE is_overridden = 0", [], |r| r.get(0));
    conn.execute("DELETE FROM ads WHERE rowid IN (
                      SELECT rowid FROM ads ORDER BY stored_at LIMIT 1)", []);
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 5);
    }

    #[test]
    fn ignores_ddl_migrations_and_strings_not_run() {
        let code = r#"
const SQL: &str = "create table t(a)";
fn upgrade_from(tx: &Transaction) {
    tx.execute_batch("insert into t select a from u").unwrap();
}
fn f(conn: &Connection) {
    conn.execute_batch("create index i on t(a)").unwrap();
    let message = "select from the list";
}
"#;
        let found = check(&RULE, &[(PATH, code)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/sql-formatting/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
