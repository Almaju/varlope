pub fn run_maintenance(conn: &PlacesDb) -> Result<()> {
    prune_history(conn)?;
    sql_support::maintenance::vacuum(conn)?;
    Ok(())
}
