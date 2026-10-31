impl NimbusClient {
    /// Opts the user in or out of experiments and rollouts.
    pub fn set_participation(&self, experiments: bool, rollouts: bool) { // flagged
        // …
    }
}

fn on_settings_changed(client: &NimbusClient) {
    client.set_participation(true, false); // which flag is which?
}
