pub fn run_maintenance_vacuum(conn: &Connection) -> Result<()> {
    // flagged in review: copy of sql_support::maintenance::vacuum
    let auto_vacuum: u32 = conn.conn_ext_query_one("PRAGMA auto_vacuum")?;
    if auto_vacuum == 2 {
        conn.execute_one("PRAGMA incremental_vacuum(2)")?;
    } else {
        conn.execute_one("PRAGMA auto_vacuum=incremental")?;
        conn.execute_one("VACUUM")?;
    }
    Ok(())
}
