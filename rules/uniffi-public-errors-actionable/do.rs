#[derive(Debug, thiserror::Error)]
pub enum StoreApiError {
    #[error("No record with that id: {reason}")]
    NoSuchRecord { reason: String },
    #[error("Unexpected error: {reason}")]
    Unexpected { reason: String },
}

// in get_error_handling():
Error::Json(e) => ErrorHandling::convert(StoreApiError::Unexpected {
    reason: e.to_string(),
})
.report_error("store-json-parse"),
