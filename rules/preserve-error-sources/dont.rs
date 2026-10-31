#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error opening database: {0}")]
    OpenDatabase(String),
    // …
}

impl Store {
    pub fn close(&self) -> Result<()> {
        // flagged: rusqlite error stringified into an unrelated variant
        self.db.close().map_err(|e| Error::OpenDatabase(e.to_string()))
    }
}
