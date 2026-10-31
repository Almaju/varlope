#[uniffi::export]
impl Store {
    /// Updates the record with the given `id`.
    ///
    /// Returns the updated record.
    #[handle_error(Error)]
    pub fn update(&self, id: String, fields: RecordFields) -> ApiResult<Record> {
        // …
    }
}
