impl SuggestDao<'_> {
    pub fn insert_dismissal(&self, key: &str) -> Result<()> { // flagged: writes through `&self`
        // flagged: `execute` runs only the INSERT; the UPDATE is silently skipped
        self.conn.execute(
            "INSERT OR IGNORE INTO dismissed_suggestions(url) VALUES(:url);
             UPDATE meta SET value = value + 1 WHERE key = 'dismissal_count';",
            named_params! { ":url": key },
        )?;
        Ok(())
    }
}
