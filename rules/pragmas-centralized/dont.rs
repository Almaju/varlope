fn prepare(&self, conn: &Connection, _db_empty: bool) -> open_database::Result<()> {
    sql_support::setup_sqlite_defaults(conn)?;
    Ok(())
}

pub fn delete_all_credit_cards(conn: &Connection) -> Result<()> {
    // flagged: PRAGMA outside ConnectionInitializer::prepare, not in db-pragmas.md
    conn.execute_batch("PRAGMA secure_delete = true;")?;
    conn.execute_cached("DELETE FROM credit_cards_data", [])?;
    Ok(())
}
