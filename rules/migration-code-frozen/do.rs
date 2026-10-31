pub fn upgrade_from(db: &Connection, from: u32) -> rusqlite::Result<()> {
    match from {
        // …
        20 => {
            db.execute_batch(
                "CREATE INDEX IF NOT EXISTS visits_type_date_idx
                 ON moz_historyvisits(visit_type, visit_date);",
            )?;
            db.execute("ANALYZE moz_historyvisits", [])?;
        }
        _ => panic!("Places does not have a v{} -> v{} migration", from, from + 1),
    }
    Ok(())
}
