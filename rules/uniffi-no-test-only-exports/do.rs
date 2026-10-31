#[cfg(any(test, feature = "testing"))]
impl Store {
    /// Wipes all data and resets the schema.
    pub fn reset_for_tests(&self) {
        self.db.lock().wipe_all();
    }
}
