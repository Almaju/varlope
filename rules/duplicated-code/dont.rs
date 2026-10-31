pub fn run_maintenance(conn: &PlacesDb) -> Result<()> {
    prune_history(conn)?;
    // flagged: copied from `sql_support::maintenance::vacuum`
    let auto_vacuum: u32 = conn.query_one("PRAGMA auto_vacuum")?;
    if auto_vacuum != 2 {
        conn.execute_one("PRAGMA auto_vacuum = incremental")?;
        conn.execute_one("VACUUM")?;
    }
    conn.execute_one("PRAGMA incremental_vacuum")?;
    conn.execute_one("PRAGMA optimize")?;
    conn.execute_one("PRAGMA wal_checkpoint(PASSIVE)")?;
    Ok(())
}
