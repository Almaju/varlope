#[uniffi::export]
impl Store {
    /// Returns the item with `id`, if any.
    #[handle_error(Error)]
    pub fn get(&self, id: &str) -> ApiResult<Option<Item>> {
        Ok(self.db.lock().get_by_id(id)?)
    }
}

impl GetErrorHandling for Error {
    type ExternalError = StoreApiError;
    fn get_error_handling(&self) -> ErrorHandling<StoreApiError> {
        ErrorHandling::convert(StoreApiError::Unexpected { reason: self.to_string() })
            .report_error("store-unexpected")
    }
}
