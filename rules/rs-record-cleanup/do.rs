pub const SQL: &str = "
CREATE TABLE yelp_modifiers(
    keyword TEXT PRIMARY KEY,
    type INTEGER NOT NULL,
    record_id TEXT NOT NULL
) WITHOUT ROWID;
";
impl SuggestDao<'_> {
    pub fn drop_suggestions(&mut self, record_id: &SuggestRecordId) -> Result<()> {
        let params = named_params! { ":record_id": record_id.as_str() };
        self.conn.execute_cached("DELETE FROM suggestions WHERE record_id = :record_id", params)?;
        self.conn.execute_cached("DELETE FROM yelp_modifiers WHERE record_id = :record_id", params)?;
        Ok(())
    }
}
