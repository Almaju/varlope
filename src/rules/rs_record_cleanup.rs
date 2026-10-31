//! Tables filled from Remote Settings records carry `record_id` and are cleaned on deletion
//!
//! Docs and examples: `rules/rs-record-cleanup/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::sql::{self, SqlText, Tok};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "rs-record-cleanup",
    area: Area::Sql,
    title:
        "Tables filled from Remote Settings records carry `record_id` and are cleaned on deletion",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let texts = sql::sql_texts(cx.ws);
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
        orphaned_tables(texts, out);
        deletes_in_reads(texts, out);
    }
}

/// In a crate that deletes data per record (`DELETE … WHERE record_id = …`),
/// tables that ingestion fills but nothing ever deletes from: their rows
/// outlive the record they came from.
fn orphaned_tables(texts: &[&SqlText], out: &mut Findings) {
    let code: Vec<&&SqlText> = texts
        .iter()
        .filter(|t| !t.in_test && !t.in_migration())
        .collect();
    let mut per_record = false;
    let mut inserted: BTreeSet<String> = BTreeSet::new();
    // Tables each function deletes from, and whether every delete there is
    // unconditional: a function that empties several tables wipes the whole
    // database (`clear_database`), which is no cleanup after one record.
    let mut deletes: BTreeMap<(String, usize), (BTreeSet<String>, bool)> = BTreeMap::new();
    for text in &code {
        let key = (
            text.path.clone(),
            text.func
                .as_ref()
                .map(|f| f.start_line)
                .unwrap_or(text.line),
        );
        for stmt in text.statements() {
            let toks = &stmt.toks;
            if stmt.verb() == "DELETE" {
                per_record |= toks.iter().any(|t| t.is("record_id"));
                let entry = deletes
                    .entry(key.clone())
                    .or_insert((BTreeSet::new(), true));
                entry.0.extend(table_after(toks, "FROM"));
                entry.1 &= !toks.iter().any(|t| t.is("WHERE"));
            }
            if stmt.verb() == "INSERT" || stmt.verb() == "REPLACE" {
                inserted.extend(table_after(toks, "INTO"));
            }
        }
    }
    let deleted: BTreeSet<String> = deletes
        .into_values()
        .filter(|(tables, unconditional)| !(*unconditional && tables.len() >= 3))
        .flat_map(|(tables, _)| tables)
        .collect();
    if !per_record {
        return;
    }
    let schema = sql::schema_from(texts);
    // Rows that go away with a deleted parent, or through a delete trigger.
    let mut cleaned = deleted.clone();
    loop {
        let before = cleaned.len();
        for (table, _) in &schema.tables {
            let cascades = table.foreign_keys.iter().any(|fk| {
                fk.on_delete.as_deref() == Some("CASCADE")
                    && cleaned.contains(&fk.parent.to_ascii_lowercase())
            });
            if cascades {
                cleaned.insert(table.name.to_ascii_lowercase());
            }
        }
        for (trigger, _) in &schema.triggers {
            if trigger.event == "DELETE" && cleaned.contains(&trigger.table.to_ascii_lowercase()) {
                cleaned.extend(trigger.deletes_from.iter().map(|t| t.to_ascii_lowercase()));
            }
        }
        if cleaned.len() == before {
            break;
        }
    }
    for (table, text) in &schema.tables {
        let name = table.name.to_ascii_lowercase();
        if table.temp || is_key_value(&name) || !inserted.contains(&name) || cleaned.contains(&name)
        {
            continue;
        }
        out.add(
            &text.path,
            text.line_at(table.start),
            format!(
                "ingestion fills `{}` but nothing deletes its rows when the record they came from is deleted; delete them in the record-deletion path (e.g. `drop_suggestions()`)",
                table.name
            ),
        );
    }
}

/// Key/value bookkeeping, not record data.
fn is_key_value(name: &str) -> bool {
    name == "meta" || name.ends_with("_meta") || name.ends_with("meta_data")
}

/// The lowercase table name after `keyword` (`DELETE FROM t`, `INSERT … INTO t`).
fn table_after(toks: &[Tok], keyword: &str) -> Option<String> {
    let at = toks.iter().position(|t| t.is(keyword))?;
    let mut i = at + 1;
    if toks.get(i + 1).is_some_and(|t| t.is_punct(".")) {
        i += 2;
    }
    let t = toks.get(i)?;
    (t.kind != sql::TokKind::Hole).then(|| t.name().to_ascii_lowercase())
}

/// A function that reads (`get_*`, `decrypt_*`…) but deletes rows: a read
/// with a persistent side effect, which can wipe data on a transient error.
fn deletes_in_reads(texts: &[&SqlText], out: &mut Findings) {
    for text in texts.iter().filter(|t| !t.in_test) {
        let Some(func) = &text.func else { continue };
        let reads = [
            "get_", "decrypt", "list_", "find_", "fetch_", "read_", "load_",
        ]
        .iter()
        .any(|p| func.name.starts_with(p));
        if !reads {
            continue;
        }
        for stmt in text.statements() {
            if stmt.verb() == "DELETE" {
                out.add(
                    &text.path,
                    text.line_at(stmt.start()),
                    format!(
                        "`{}` reads, yet deletes rows; move the delete to an explicit write path so a failed read cannot destroy data",
                        func.name
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const DB: (&str, &str) = (
        "components/suggest/src/db.rs",
        r#"
impl SuggestDao<'_> {
    pub fn drop_suggestions(&mut self, record_id: &SuggestRecordId) -> Result<()> {
        self.conn.execute_cached("DELETE FROM suggestions WHERE record_id = :record_id", [])?;
        self.conn.execute_cached("DELETE FROM yelp_subjects WHERE record_id = :record_id", [])?;
        Ok(())
    }
    pub fn insert_yelp(&mut self, record_id: &SuggestRecordId) -> Result<()> {
        self.conn.execute_cached("INSERT INTO yelp_subjects(keyword, record_id) VALUES(?, ?)", [])?;
        self.conn.execute_cached("INSERT INTO yelp_modifiers(type, keyword) VALUES(?, ?)", [])?;
        self.conn.execute_cached("INSERT INTO suggestions(record_id) VALUES(?)", [])?;
        self.conn.execute_cached("INSERT INTO amp_custom_details(suggestion_id) VALUES(?)", [])?;
        self.conn.execute_cached("INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)", [])?;
        Ok(())
    }
}
"#,
    );

    fn schema(sql: &str) -> String {
        format!(
            "pub const SQL: &str = \"\nCREATE TABLE suggestions(id INTEGER PRIMARY KEY, record_id TEXT NOT NULL);\n\
             CREATE TABLE amp_custom_details(suggestion_id INTEGER PRIMARY KEY REFERENCES suggestions(id) ON DELETE CASCADE);\n\
             CREATE TABLE yelp_subjects(keyword TEXT PRIMARY KEY, record_id TEXT NOT NULL);\n\
             CREATE TABLE meta(key TEXT PRIMARY KEY, value NOT NULL);\n{sql}\";\n"
        )
    }

    /// Ingestion, elsewhere in the crate.
    const INGEST: (&str, &str) = (
        "components/suggest/src/yelp.rs",
        "impl SuggestDao<'_> {\n    fn insert_yelp(&mut self) -> Result<()> {\n        \
         self.conn.execute_cached(\"INSERT INTO yelp_modifiers(record_id, type, keyword) VALUES(?, ?, ?)\", [])?;\n        \
         Ok(())\n    }\n}\n",
    );

    #[test]
    fn flags_dont() {
        let dont = include_str!("../../rules/rs-record-cleanup/dont.rs");
        let found = check(&RULE, &[INGEST, ("components/suggest/src/schema.rs", dont)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn accepts_do() {
        let fixed = include_str!("../../rules/rs-record-cleanup/do.rs");
        let found = check(
            &RULE,
            &[INGEST, ("components/suggest/src/schema.rs", fixed)],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn cascades_triggers_and_key_value_tables_are_fine() {
        let found = check(
            &RULE,
            &[DB, ("components/suggest/src/schema.rs", &schema(""))],
        );
        assert!(found.is_empty(), "{found:#?}");
        let trigger = "CREATE TABLE yelp_modifiers(type INTEGER, keyword TEXT, subject TEXT);\n\
                       CREATE TRIGGER yelp_subjects_delete AFTER DELETE ON yelp_subjects BEGIN\n\
                       DELETE FROM yelp_modifiers WHERE subject = old.keyword;\nEND;";
        let found = check(
            &RULE,
            &[DB, ("components/suggest/src/schema.rs", &schema(trigger))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn crates_without_per_record_deletes_are_skipped() {
        let db = DB.1.replace("WHERE record_id = :record_id", "");
        let found = check(
            &RULE,
            &[
                (DB.0, &db),
                (
                    "components/suggest/src/schema.rs",
                    &schema("CREATE TABLE yelp_modifiers(type INTEGER);"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_deletes_in_reads() {
        let code = r#"
impl LoginDb {
    fn decrypt_fields(&self, guid: &str) -> Result<Login> {
        self.execute("DELETE FROM loginsL WHERE guid = ?", [guid])?;
        todo!()
    }
}
"#;
        let found = check(&RULE, &[("components/logins/src/db.rs", code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
    }
}
