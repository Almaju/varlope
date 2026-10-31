#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error sending request: {0}")]
    RequestError(#[from] viaduct::ViaductError),
}

fn to_api_error(error: &Error) -> ApiError {
    let reason = error.to_string();
    match error {
        Error::RequestError(_) => ApiError::Network { reason },
    }
}
