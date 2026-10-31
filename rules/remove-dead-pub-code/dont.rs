#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error sending request: {0}")]
    RequestError(#[from] viaduct::ViaductError),
    #[error("Error in HTTP response: {0}")]
    HttpError(String), // flagged: matched below, never constructed
}

fn to_api_error(error: &Error) -> ApiError {
    let reason = error.to_string();
    match error {
        Error::RequestError(_) => ApiError::Network { reason },
        Error::HttpError(_) => ApiError::Other { reason },
    }
}
