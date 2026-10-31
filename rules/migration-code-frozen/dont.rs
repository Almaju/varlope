pub fn upgrade_from(db: &Connection, from: u32) -> rusqlite::Result<()> {
    match from {
        // …
        20 => {
            // flagged: replays today's shared schema, not the v21 DDL
            db.execute_batch(CREATE_SHARED_SCHEMA_SQL)?;
            db.execute("ANALYZE moz_historyvisits", [])?;
        }
        _ => panic!("Places does not have a v{} -> v{} migration", from, from + 1),
    }
    Ok(())
}
