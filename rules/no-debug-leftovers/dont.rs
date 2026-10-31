impl SuggestDao<'_> {
    fn ingest_record(&mut self, record: &Record) -> Result<()> {
        dbg!(&record.id); // flagged: dbg! in shipped code
        let suggestions = self.download_attachment(record)?;
        println!("ingesting {} suggestions", suggestions.len()); // flagged
        self.insert_suggestions(&record.id, &suggestions)
    }
}
