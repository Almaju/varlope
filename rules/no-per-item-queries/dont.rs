impl SuggestDao<'_> {
    fn insert_yelp_subjects(&mut self, record_id: &SuggestRecordId, subjects: &[Subject]) -> Result<()> {
        self.scope.err_if_interrupted()?;
        for subject in subjects { // flagged: writes without checking for interruption
            // flagged: the statement is prepared again for every subject
            let mut stmt = self.conn.prepare(
                "INSERT INTO yelp_subjects(keyword, subject_type, record_id) VALUES(?, ?, ?)",
            )?;
            stmt.execute((&subject.keyword, subject.subject_type as u8, record_id.as_str()))?;
        }
        Ok(())
    }
}
