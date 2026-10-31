impl ConnectionInitializer for AutofillConnectionInitializer {
    fn prepare(&self, conn: &Connection, _db_empty: bool) -> open_database::Result<()> {
        sql_support::setup_sqlite_defaults(conn)?;
        Ok(())
    }
    // …
}

pub fn delete_all_credit_cards(conn: &Connection) -> Result<()> {
    // Only the connection that ran this has secure_delete on.
    conn.execute_batch("PRAGMA secure_delete = true;")?; // flagged: set outside `prepare`
    conn.execute_cached("DELETE FROM credit_cards_data", [])?;
    Ok(())
}
