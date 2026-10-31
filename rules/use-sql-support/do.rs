impl SuggestDao<'_> {
    pub fn insert_dismissal(&mut self, key: &str) -> Result<()> {
        self.conn.execute_cached(
            "INSERT OR IGNORE INTO dismissed_suggestions(url) VALUES(:url)",
            named_params! { ":url": key },
        )?;
        self.conn.execute_cached(
            "UPDATE meta SET value = value + 1 WHERE key = 'dismissal_count'",
            [],
        )?;
        Ok(())
    }
}
