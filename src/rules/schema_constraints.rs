//! Columns declare NOT NULL/keys, FKs declare ON DELETE and are indexed, new indexes are justified
//!
//! Docs and examples: `rules/schema-constraints/`.

use crate::helpers::sql::{self, Schema, Table};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "schema-constraints",
    area: Area::Sql,
    title: "Columns declare NOT NULL/keys, FKs declare ON DELETE and are indexed, new indexes are justified",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let texts = sql::sql_texts(cx.ws);
    for texts in sql::by_package(cx.ws, &texts).values() {
        let schema = sql::schema_from(texts);
        for (table, text) in &schema.tables {
            if table.temp || table.virtual_table {
                continue;
            }
            for fk in &table.foreign_keys {
                let line = text.line_at(fk.start);
                let cols = fk.columns.join(", ");
                if fk.on_delete.is_none() {
                    out.add(
                        &text.path,
                        line,
                        format!(
                            "`{}({cols})` references `{}` without `ON DELETE`; say what deleting the parent does (`CASCADE`, `SET NULL` or `RESTRICT`)",
                            table.name, fk.parent
                        ),
                    );
                }
                if !indexed(&schema, table, &fk.columns) {
                    out.add(
                        &text.path,
                        line,
                        format!(
                            "foreign key `{}({cols})` has no index starting with it, so every delete from `{}` scans `{}`; add an index",
                            table.name, fk.parent, table.name
                        ),
                    );
                }
            }
        }
    }
}

/// Whether `columns` are the leading columns of the primary key, a `UNIQUE`
/// constraint or an index of `table`.
fn indexed(schema: &Schema, table: &Table, columns: &[String]) -> bool {
    let leads = |cols: &[String]| {
        cols.len() >= columns.len()
            && cols
                .iter()
                .zip(columns)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    let rowid_alias = columns.len() == 1
        && table
            .column(&columns[0])
            .is_some_and(|c| c.primary_key && c.ty == "INTEGER");
    rowid_alias
        || leads(&table.primary_key)
        || table.unique.iter().any(|u| leads(u))
        || schema
            .indexes
            .iter()
            .any(|(i, _)| i.table.eq_ignore_ascii_case(&table.name) && leads(&i.columns))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/sql/create_schema.sql";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/schema-constraints/dont.sql"),
            )],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 6);
        assert!(found.iter().any(|f| f.message.contains("ON DELETE")));
        assert!(found.iter().any(|f| f.message.contains("no index")));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/schema-constraints/do.sql"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_tests_migrations_and_temp_tables() {
        let rust = r#"
fn upgrade_from(tx: &Transaction, version: u32) {
    tx.execute_batch("CREATE TABLE a(p INTEGER REFERENCES b(id))").unwrap();
}
fn finish(conn: &Connection) {
    conn.execute_batch("CREATE TEMP TABLE c(p INTEGER REFERENCES b(id))").unwrap();
}
#[cfg(test)]
mod tests {
    const V1: &str = "CREATE TABLE d(p INTEGER REFERENCES b(id))";
}
"#;
        let found = check(
            &RULE,
            &[
                ("components/x/src/schema.rs", rust),
                (
                    "components/x/sql/tests/create_v1_db.sql",
                    "CREATE TABLE e(p INTEGER REFERENCES b(id));",
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn primary_keys_and_rowid_aliases_count_as_indexes() {
        let sql = "CREATE TABLE d(suggestion_id INTEGER PRIMARY KEY REFERENCES s(id) ON DELETE CASCADE);\n\
                   CREATE TABLE k(a TEXT, s INTEGER NOT NULL, PRIMARY KEY (s, a), FOREIGN KEY(s) REFERENCES s(id) ON DELETE CASCADE);";
        assert!(check(&RULE, &[(PATH, sql)]).is_empty());
    }
}
