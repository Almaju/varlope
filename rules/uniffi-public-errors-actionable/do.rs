#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StoreApiError {
    #[error("Unexpected error: {reason}")]
    Unexpected { reason: String },
}

impl GetErrorHandling for Error {
    type ExternalError = StoreApiError;

    fn get_error_handling(&self) -> ErrorHandling<StoreApiError> {
        ErrorHandling::convert(StoreApiError::Unexpected { reason: self.to_string() })
            .report_error("store-unexpected")
    }
}
