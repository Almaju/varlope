#[uniffi::export]
impl Store {
    /// Returns the item with `id`, if any.
    pub fn get(&self, id: &str) -> ApiResult<Option<Item>> { // flagged: no handle_error
        Ok(self.db.lock().get_by_id(id)?)
    }
}

impl From<Error> for StoreApiError { // flagged: convert via GetErrorHandling
    fn from(e: Error) -> Self {
        StoreApiError::Unexpected { reason: e.to_string() }
    }
}
