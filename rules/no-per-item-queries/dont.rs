for subject in subjects {
    // flagged: statement prepared per item; loop never checks for interruption
    let mut stmt = self.conn.prepare(
        "INSERT INTO yelp_subjects(keyword, subject_type, record_id) VALUES(?, ?, ?)",
    )?;
    stmt.execute((&subject.keyword, subject.subject_type as u8, record_id.as_str()))?;
}
