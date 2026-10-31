impl ConnectionInitializer for AutofillConnectionInitializer {
    fn prepare(&self, conn: &Connection, _db_empty: bool) -> open_database::Result<()> {
        sql_support::setup_sqlite_defaults(conn)?;
        // Listed in docs/design/db-pragmas.md.
        conn.execute_batch("PRAGMA secure_delete = true;")?;
        Ok(())
    }
    // …
}

pub fn delete_all_credit_cards(conn: &Connection) -> Result<()> {
    conn.execute_cached("DELETE FROM credit_cards_data", [])?;
    Ok(())
}
