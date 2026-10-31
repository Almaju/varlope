#[uniffi::export]
impl Store {
    /// Returns the item with `id`, if any.
    pub fn get(&self, id: &str) -> ApiResult<Option<Item>> { // flagged: built by hand
        self.db.lock().get_by_id(id).map_err(|e| StoreApiError::Unexpected {
            reason: e.to_string(),
        })
    }
}

impl From<Error> for StoreApiError { // flagged: bypasses GetErrorHandling
    fn from(e: Error) -> Self {
        StoreApiError::Unexpected { reason: e.to_string() }
    }
}
