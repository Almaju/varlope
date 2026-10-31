#[uniffi::export]
impl Store {
    /// Wipes all data and resets the schema. Only for tests.
    pub fn reset_for_tests(&self) { // flagged: test-only helper exported over FFI
        self.db.lock().wipe_all();
    }
}
