#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error sending request: {0}")]
    RequestError(#[from] viaduct::ViaductError),
}

impl GetErrorHandling for Error {
    type ExternalError = ApiError;

    fn get_error_handling(&self) -> ErrorHandling<ApiError> {
        let reason = self.to_string();
        match self {
            Self::RequestError(_) => ErrorHandling::convert(ApiError::Network { reason }),
        }
    }
}
