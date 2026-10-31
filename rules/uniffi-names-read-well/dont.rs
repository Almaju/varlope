#[uniffi::export]
impl LoginStore {
    /// Checks every record can be decrypted; deletes the ones that can't.
    #[handle_error(Error)]
    pub fn verify_records(&self) -> ApiResult<u64> { // flagged: deletes, but says "verify"
        todo!()
    }

    /// Runs a sync and waits for it to finish.
    pub fn run_blocking(&self) { // flagged: means something else in Kotlin
        todo!()
    }
}
