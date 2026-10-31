#[uniffi::export]
impl Store {
    /// Updates the record with the given `id`, and returns
    // the updated record. // flagged: `//` drops this line from the docs
    #[handle_error(Error)]
    pub fn update(&self, id: String, fields: RecordFields) -> ApiResult<Record> {
        self.db.lock().update(&id, fields)
    }
}
