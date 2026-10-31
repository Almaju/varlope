use error_support::trace;

impl SuggestDao<'_> {
    fn ingest_record(&mut self, record: &Record) -> Result<()> {
        let suggestions = self.download_attachment(record)?;
        trace!("ingesting {} suggestions from {}", suggestions.len(), record.id);
        self.insert_suggestions(&record.id, &suggestions)
    }
}
