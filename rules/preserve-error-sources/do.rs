#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
}

impl Store {
    pub fn close(&self) -> Result<()> {
        Ok(self.db.close()?)
    }
}
