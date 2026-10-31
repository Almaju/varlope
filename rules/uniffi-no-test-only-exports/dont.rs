#[uniffi::export]
impl RemoteCommandStore {
    /// Adds a command with an explicit timestamp. Primarily used by tests.
    pub fn add_remote_command_at(&self, device_id: &str, when: Timestamp) -> bool { // flagged
        self.add_at(device_id, when)
    }
}
