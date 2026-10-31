pub fn record_events(&self, events: &[Event]) -> Result<()> {
    let db = self.db.lock();
    for event in events {
        db.execute_cached(INSERT_EVENT_SQL, (&event.id, event.timestamp))?;
    }
    Ok(())
}
