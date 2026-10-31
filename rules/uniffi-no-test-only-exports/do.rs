#[uniffi::export]
impl RemoteCommandStore {
    /// Adds a command, timestamped now.
    pub fn add_remote_command(&self, device_id: &str) -> bool {
        self.add_at(device_id, Timestamp::now())
    }
}

#[cfg(any(test, feature = "testing"))]
impl RemoteCommandStore {
    /// Adds a command with an explicit timestamp, for tests.
    pub fn add_remote_command_at(&self, device_id: &str, when: Timestamp) -> bool {
        self.add_at(device_id, when)
    }
}
