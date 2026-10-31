use error_support::trace;

fn ingest_record(&mut self, record: &RemoteSettingsRecord) -> Result<()> {
    let suggestions = self.download_attachment(record)?;
    trace!("ingesting {} suggestions from {}", suggestions.len(), record.id);
    self.dao.insert_suggestions(&record.id, &suggestions)
}
