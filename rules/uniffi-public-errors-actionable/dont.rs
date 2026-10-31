#[derive(Debug, thiserror::Error)]
pub enum StoreApiError {
    #[error("No record with that id: {reason}")]
    NoSuchRecord { reason: String },
    #[error("JSON parse error")]
    JsonParseError, // flagged: callers can't act on this
    #[error("Unexpected error")]
    Unexpected, // flagged: catch-all without a reason
}

// in get_error_handling():
_ => ErrorHandling::convert(StoreApiError::Unexpected), // flagged: wildcard arm
