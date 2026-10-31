#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StoreApiError {
    #[error("JSON error: {reason}")]
    Json { reason: String }, // flagged: nothing ever returns it
    #[error("Unexpected error")]
    Unexpected, // flagged: catch-all without a reason
}

impl GetErrorHandling for Error {
    type ExternalError = StoreApiError;

    fn get_error_handling(&self) -> ErrorHandling<StoreApiError> {
        ErrorHandling::convert(StoreApiError::Unexpected) // flagged: never reported
    }
}
