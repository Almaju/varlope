fn ingest_record(&mut self, record: &RemoteSettingsRecord) -> Result<()> {
    dbg!(&record.id); // flagged: dbg! in shipped code
    let suggestions = self.download_attachment(record)?;
    println!("ingesting {} suggestions", suggestions.len()); // flagged: print_stdout
    self.dao.insert_suggestions(&record.id, &suggestions)
}
