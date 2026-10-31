impl SuggestDao<'_> {
    // flagged: write method takes &self; uncached Connection::execute
    pub fn insert_dismissal(&self, key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO dismissed_suggestions(url) VALUES(:url)",
            named_params! { ":url": key },
        )?;
        Ok(())
    }
}
