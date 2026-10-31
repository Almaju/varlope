#[uniffi::export]
impl Store {
    /// Deletes every record that can no longer be decrypted; returns how many.
    #[handle_error(Error)]
    pub fn delete_undecryptable_records(&self) -> ApiResult<u64> {
        // …
    }

    /// Runs a sync and waits for it to finish.
    pub fn sync_and_wait(&self) { /* … */ }
}
