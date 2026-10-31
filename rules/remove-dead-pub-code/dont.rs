#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error sending request: {0}")]
    RequestError(#[from] viaduct::ViaductError),
    #[error("Error in HTTP response: {0}")]
    HttpError(String), // flagged: matched below, never constructed
}

impl GetErrorHandling for Error {
    type ExternalError = ApiError;

    fn get_error_handling(&self) -> ErrorHandling<ApiError> {
        let reason = self.to_string();
        match self {
            Self::RequestError(_) => ErrorHandling::convert(ApiError::Network { reason }),
            Self::HttpError(_) => ErrorHandling::convert(ApiError::Other { reason }),
        }
    }
}
