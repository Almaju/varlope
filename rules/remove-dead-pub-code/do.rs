#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error sending request: {0}")]
    RequestError(#[from] viaduct::ViaductError),
}
// impl GetErrorHandling for Error { fn get_error_handling(&self) … }
let reason = self.to_string();
match self {
    Self::RequestError(_) => ErrorHandling::convert(ApiError::Network { reason }),
}
