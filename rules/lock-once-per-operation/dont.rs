pub fn record_events(&self, events: &[Event]) -> Result<()> {
    for event in events {
        let db = self.db.lock(); // flagged: lock taken once per event
        db.execute_cached(INSERT_EVENT_SQL, (&event.id, event.timestamp))?;
    }
    Ok(())
}
