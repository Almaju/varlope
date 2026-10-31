// components/remote_settings/src/error.rs
fn get_error_handling(&self) -> ErrorHandling<RemoteSettingsError> {
    match self {
        Self::BackoffError(seconds) => {
            ErrorHandling::convert(RemoteSettingsError::Backoff { seconds: *seconds })
                .report_error("suggest-backoff") // flagged: not a remote-settings-* label
        }
        // …
    }
}
