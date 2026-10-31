impl SuggestDao<'_> {
    fn insert_yelp_subjects(&mut self, record_id: &SuggestRecordId, subjects: &[Subject]) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO yelp_subjects(keyword, subject_type, record_id) VALUES(?, ?, ?)",
        )?;
        for subject in subjects {
            self.scope.err_if_interrupted()?;
            stmt.execute((&subject.keyword, subject.subject_type as u8, record_id.as_str()))?;
        }
        Ok(())
    }
}
