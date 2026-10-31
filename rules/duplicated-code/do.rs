pub fn run_maintenance_vacuum(conn: &Connection) -> Result<()> {
    sql_support::maintenance::vacuum(conn)?;
    Ok(())
}
